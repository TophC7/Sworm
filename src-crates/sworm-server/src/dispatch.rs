use crate::{auth, events::HostEvents, pty_stream::RunFanout};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use sworm_core::{
    services::{completed_runs::CompletedRunStore, pty::PtyRunState},
    Host,
};
use sworm_protocol::{
    rpc::{Reply, Request, Response, RunStatus, WireError, MAX_REMOTE_FILE_BYTES},
    session::SessionStartInfo,
};
use sworm_remote::Fingerprint;
use tokio::sync::{watch, Mutex};

pub(crate) struct ServerContext {
    pub config_dir: PathBuf,
    pub auth_token: Option<String>,
    pub pairing: Mutex<()>,
    pub folders: parking_lot::Mutex<HashMap<PathBuf, usize>>,
    pub host_events: HostEvents,
    pub completed: Arc<CompletedRunStore>,
    /// Per-connection LSP streams, keyed by session id. A session's stream
    /// owns its server: closing it kills the process.
    pub lsp: crate::lsp_stream::LspStreams,
    runs: Mutex<HashMap<String, RunRecord>>,
}

impl ServerContext {
    pub(crate) fn new(
        config_dir: PathBuf,
        auth_token: Option<String>,
        host_events: HostEvents,
        completed: Arc<CompletedRunStore>,
    ) -> Self {
        Self {
            config_dir,
            auth_token,
            pairing: Mutex::new(()),
            folders: parking_lot::Mutex::new(HashMap::new()),
            host_events,
            completed,
            lsp: crate::lsp_stream::LspStreams::new(),
            runs: Mutex::new(HashMap::new()),
        }
    }

    pub fn claim_folder(&self, folder: &Path) {
        *self.folders.lock().entry(folder.to_path_buf()).or_insert(0) += 1;
    }

    /// Returns true when this was the last claim, so the caller releases Host resources.
    pub fn release_folder(&self, folder: &Path) -> bool {
        let mut folders = self.folders.lock();
        let Some(count) = folders.get_mut(folder) else {
            return false;
        };
        *count -= 1;
        if *count == 0 {
            folders.remove(folder);
            true
        } else {
            false
        }
    }

    pub(crate) async fn run_fanout(&self, run_id: &str) -> Option<Arc<RunFanout>> {
        self.runs
            .lock()
            .await
            .get(run_id)
            .map(|run| Arc::clone(&run.fanout))
    }
}

pub(crate) struct Session {
    pub fingerprint: Fingerprint,
    pub authorized: bool,
    pub subscriber_id: String,
    pub folders: HashSet<PathBuf>,
    pub next_events: u64,
    pub events: Option<(u64, watch::Sender<bool>)>,
}

#[derive(PartialEq, Eq)]
enum RunKind {
    Session { provider_id: String },
    Task { task_id: String },
}

struct RunRecord {
    folder: PathBuf,
    kind: RunKind,
    fanout: Arc<RunFanout>,
}

struct DispatchRuntime<'a> {
    host: &'a Host,
    context: &'a Arc<ServerContext>,
    session: &'a Mutex<Session>,
}

/// One daemon-side method per operation.
///
/// Every routed path is claimed here, so a folder the desktop reaches by any
/// op gets its watchers and its release bookkeeping. Operations whose arm
/// expands to nothing are written by hand below because they need run,
/// stream, or pairing state the table cannot express; forgetting one is a
/// compile error in `dispatch`, never a silently missing claim.
macro_rules! dispatch_operation {
    (#[route($route:ident)] FileRead => $($rest:tt)*) => {};
    (#[route($route:ident)] FileWrite => $($rest:tt)*) => {};
    (#[route($route:ident)] FolderListEntries => $($rest:tt)*) => {};
    (#[route($route:ident)] FilesWatchDirs => $($rest:tt)*) => {};
    (#[route($route:ident)] GitWatch => $($rest:tt)*) => {};
    (#[route($route:ident)] SessionStart => $($rest:tt)*) => {};
    (#[route($route:ident)] TasksStart => $($rest:tt)*) => {};
    (#[route($route:ident)] SessionStop => $($rest:tt)*) => {};
    (#[route($route:ident)] TasksStop => $($rest:tt)*) => {};
    (#[route($route:ident)] RunStatus => $($rest:tt)*) => {};
    (#[route($route:ident)] LspStart => $($rest:tt)*) => {};
    (#[route($route:ident)] Pair => $($rest:tt)*) => {};
    (
        #[route(server)]
        SettingsPatchGlobalSection => $method:ident(
            $input:ident: $input_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $input: $input_type) -> Result<$return_type, WireError> {
            reject_desktop_section(&$input.section)?;
            self.host.$method($input).await.map_err(Into::into)
        }
    };
    (
        #[route(server)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        /// The daemon is the server: its own global settings are the target.
        async fn $method(&self, $($argument: $argument_type),*) -> Result<$return_type, WireError> {
            self.host.$method($($argument),*).await.map_err(Into::into)
        }
    };
    // Session-keyed ops answer only to the connection holding that session's
    // stream lease, which the generated shape cannot check.
    (#[route(lsp)] $($rest:tt)*) => {};
    (
        #[route(opt_folder_path)]
        $variant:ident => $method:ident(
            $folder_path:ident: $folder_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $folder_path: $folder_path_type) -> Result<$return_type, WireError> {
            if let Some(folder) = $folder_path.as_deref() {
                self.claim(folder, "folder_path").await?;
            }
            self.host.$method($folder_path).await.map_err(Into::into)
        }
    };
    (
        #[route(input_folder_path)]
        $variant:ident => $method:ident(
            $input:ident: $input_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $input: $input_type) -> Result<$return_type, WireError> {
            if let Some(folder) = $input.folder_path.as_deref() {
                self.claim(folder, "folder_path").await?;
            }
            self.host.$method($input).await.map_err(Into::into)
        }
    };
    (
        #[route(input_path)]
        $variant:ident => $method:ident(
            $input:ident: $input_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $input: $input_type) -> Result<$return_type, WireError> {
            self.claim(&$input.folder_path, "folder_path").await?;
            self.host.$method($input).await.map_err(Into::into)
        }
    };
    (
        #[route($route:ident)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $($argument: $argument_type),*) -> Result<$return_type, WireError> {
            self.claim(&$route, stringify!($route)).await?;
            self.host.$method($($argument),*).await.map_err(Into::into)
        }
    };
}

macro_rules! define_dispatch {
    (
        $(
            #[route($route:ident)]
            $variant:ident => $method:ident(
                $($argument:ident: $argument_type:ty),* $(,)?
            ) -> $return_type:ty;
        )*
    ) => {
        impl DispatchRuntime<'_> {
            async fn dispatch(&self, request: Request) -> Response {
                match request {
                    $(
                        Request::$variant { $($argument),* } => self
                            .$method($($argument),*)
                            .await
                            .map(Reply::$variant),
                    )*
                }
            }

            $(
                dispatch_operation! {
                    #[route($route)]
                    $variant => $method(
                        $($argument: $argument_type),*
                    ) -> $return_type;
                }
            )*
        }
    };
}

sworm_protocol::sworm_rpc_ops!(define_dispatch);

pub(crate) async fn handle(
    host: &Host,
    context: &Arc<ServerContext>,
    session: &Mutex<Session>,
    request: Request,
) -> Response {
    if !matches!(&request, Request::Pair { .. }) && !session.lock().await.authorized {
        return Err(unauthorized("client is not paired with this server"));
    }
    DispatchRuntime {
        host,
        context,
        session,
    }
    .dispatch(request)
    .await
}

impl DispatchRuntime<'_> {
    async fn claim(&self, path: &str, field: &str) -> Result<PathBuf, WireError> {
        require_absolute(path, field)?;
        let folder = PathBuf::from(path);
        self.host.watch_settings_paths(Some(&folder));
        if self.session.lock().await.folders.insert(folder.clone()) {
            self.context.claim_folder(&folder);
        }
        Ok(folder)
    }

    /// Whether a finished run still has a stored transcript to reattach to.
    fn has_transcript(&self, run_id: &str) -> Result<bool, WireError> {
        self.context
            .completed
            .get(run_id)
            .map(|run| run.is_some())
            .map_err(|message| WireError::Internal { message })
    }

    async fn file_read(
        &self,
        project_path: String,
        file_path: String,
    ) -> Result<sworm_protocol::files::FileContent, WireError> {
        self.claim(&project_path, "project_path").await?;
        self.host
            .file_read_limited(project_path, file_path, MAX_REMOTE_FILE_BYTES)
            .await
            .map_err(Into::into)
    }

    /// Writes mirror the read ceiling: a file too large to read back is not
    /// one the daemon will store either.
    async fn file_write(
        &self,
        project_path: String,
        file_path: String,
        content: String,
        expected_version: Option<String>,
    ) -> Result<String, WireError> {
        if content.len() > MAX_REMOTE_FILE_BYTES {
            return Err(WireError::InvalidArgument {
                message: format!(
                    "File {file_path} exceeds the {MAX_REMOTE_FILE_BYTES}-byte write limit"
                ),
            });
        }
        self.claim(&project_path, "project_path").await?;
        self.host
            .file_write(project_path, file_path, content, expected_version)
            .await
            .map_err(Into::into)
    }

    /// Browsing is not ownership: the folder switcher walks directories the
    /// desktop never opens, so this claims nothing.
    async fn folder_list_entries(
        &self,
        path: String,
        show_hidden: bool,
    ) -> Result<Vec<sworm_protocol::folder::FolderEntry>, WireError> {
        require_absolute(&path, "path")?;
        self.host
            .folder_list_entries(path, show_hidden)
            .await
            .map_err(Into::into)
    }

    async fn files_watch_dirs(
        &self,
        project_path: String,
        dirs: Vec<String>,
    ) -> Result<(), WireError> {
        self.claim(&project_path, "project_path").await?;
        let subscriber = self.session.lock().await.subscriber_id.clone();
        self.host
            .files_watch_dirs(subscriber, project_path, dirs)
            .await
            .map_err(Into::into)
    }

    async fn git_watch(&self, project_path: String) -> Result<(), WireError> {
        let folder = self.claim(&project_path, "project_path").await?;
        let context = Arc::clone(self.context);
        self.host
            .git_watch(project_path, move |_| {
                context.folders.lock().contains_key(&folder)
            })
            .await
            .map_err(Into::into)
    }

    async fn session_start(
        &self,
        run_id: String,
        folder_path: String,
        provider_id: String,
        resume_token: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<SessionStartInfo, WireError> {
        let folder = self.claim(&folder_path, "folder_path").await?;
        // The runs lock spans the spawn so a second start cannot race the same
        // run id. Starts serialize per daemon; a reservation state would only
        // buy latency nobody waits on.
        let mut runs = self.context.runs.lock().await;
        // A finished run is reattachable from its transcript; respawning it
        // would silently replace output the desktop came back to read.
        let stored = self.has_transcript(&run_id)?;
        if let Some(run) = runs.get(&run_id) {
            ensure_run(
                &run_id,
                run,
                &folder,
                &RunKind::Session {
                    provider_id: provider_id.clone(),
                },
            )?;
            if stored || self.host.pty.run_state(&run_id).is_some() {
                return Ok(SessionStartInfo {
                    resumed: true,
                    resume_token: run.fanout.resume_token(),
                });
            }
            run.fanout.cancel();
            runs.remove(&run_id);
        } else if self.host.pty.run_state(&run_id).is_some() {
            return Err(conflicting_run(&run_id));
        } else if stored {
            return Ok(SessionStartInfo {
                resumed: true,
                resume_token: None,
            });
        }

        let fanout = Arc::new(RunFanout::new());
        let (output, events) = fanout.sinks();
        let info = self
            .host
            .session_start(
                run_id.clone(),
                folder_path,
                provider_id.clone(),
                resume_token,
                cols,
                rows,
                output,
                events,
                None,
                true,
            )
            .await
            .map_err(WireError::from)?;
        fanout.set_resume_token(info.resume_token.clone());
        runs.insert(
            run_id,
            RunRecord {
                folder,
                kind: RunKind::Session { provider_id },
                fanout,
            },
        );
        Ok(info)
    }

    async fn tasks_start(
        &self,
        run_id: String,
        folder_path: String,
        task_id: String,
        active_file_path: Option<String>,
        cols: u16,
        rows: u16,
    ) -> Result<(), WireError> {
        let folder = self.claim(&folder_path, "folder_path").await?;
        let mut runs = self.context.runs.lock().await;
        let stored = self.has_transcript(&run_id)?;
        if let Some(run) = runs.get(&run_id) {
            ensure_run(
                &run_id,
                run,
                &folder,
                &RunKind::Task {
                    task_id: task_id.clone(),
                },
            )?;
            if stored || self.host.pty.run_state(&run_id).is_some() {
                return Ok(());
            }
            run.fanout.cancel();
            runs.remove(&run_id);
        } else if self.host.pty.run_state(&run_id).is_some() {
            return Err(conflicting_run(&run_id));
        } else if stored {
            return Ok(());
        }

        let fanout = Arc::new(RunFanout::new());
        let (output, events) = fanout.sinks();
        self.host
            .tasks_start(
                run_id.clone(),
                folder_path,
                task_id.clone(),
                active_file_path,
                cols,
                rows,
                output,
                events,
                None,
                true,
            )
            .await
            .map_err(WireError::from)?;
        runs.insert(
            run_id,
            RunRecord {
                folder,
                kind: RunKind::Task { task_id },
                fanout,
            },
        );
        Ok(())
    }

    async fn session_stop(&self, run_id: String) -> Result<(), WireError> {
        let mut runs = self.context.runs.lock().await;
        if let Some(run) = runs.get(&run_id) {
            if !matches!(&run.kind, RunKind::Session { .. }) {
                return Err(conflicting_run(&run_id));
            }
            run.fanout.cancel();
        }
        self.host
            .session_stop(run_id.clone())
            .await
            .map_err(WireError::from)?;
        runs.remove(&run_id);
        // An explicitly stopped run is a closed tab: its transcript is moot.
        self.context.completed.delete(&run_id);
        Ok(())
    }

    async fn tasks_stop(&self, run_id: String) -> Result<(), WireError> {
        let mut runs = self.context.runs.lock().await;
        if let Some(run) = runs.get(&run_id) {
            if !matches!(&run.kind, RunKind::Task { .. }) {
                return Err(conflicting_run(&run_id));
            }
            run.fanout.cancel();
        }
        self.host
            .tasks_stop(run_id.clone())
            .await
            .map_err(WireError::from)?;
        runs.remove(&run_id);
        self.context.completed.delete(&run_id);
        Ok(())
    }

    async fn run_status(&self, run_id: String) -> Result<RunStatus, WireError> {
        Ok(match self.host.pty.run_state(&run_id) {
            Some(PtyRunState::Live) => RunStatus {
                live: true,
                exited: None,
            },
            Some(PtyRunState::Completed(code)) => RunStatus {
                live: false,
                exited: Some(code),
            },
            None => match self.context.completed.get(&run_id) {
                Ok(Some(completed)) => RunStatus {
                    live: false,
                    exited: Some(completed.exit_code),
                },
                Ok(None) => RunStatus {
                    live: false,
                    exited: None,
                },
                Err(error) => return Err(WireError::Internal { message: error }),
            },
        })
    }

    async fn lsp_start(
        &self,
        session_id: String,
        folder_path: String,
        server_definition_id: String,
        root_path: String,
    ) -> Result<(), WireError> {
        self.claim(&folder_path, "folder_path").await?;
        require_absolute(&root_path, "root_path")?;
        // The session's stream owns the server's lifetime, so a start without
        // one would spawn a process nobody can reach or kill, and only the
        // connection holding that stream may start it.
        let owner = self.session.lock().await.subscriber_id.clone();
        let lease = self.context.lsp.sink(&session_id, &owner).ok_or_else(|| {
            WireError::InvalidArgument {
                message: format!("no LSP stream is open for session {session_id}"),
            }
        })?;
        self.host
            .lsp_start(
                Some(owner),
                session_id.clone(),
                folder_path,
                server_definition_id,
                root_path,
                lease.events,
            )
            .await?;
        // The stream can close while the server is starting; a process whose
        // lease is gone answers to nobody, so stop it instead of leaking it.
        if !self.context.lsp.is_current(&session_id, lease.token) {
            let _ = self.host.lsp_stop(session_id).await;
            return Err(WireError::InvalidArgument {
                message: "LSP stream closed before its server started".to_owned(),
            });
        }
        Ok(())
    }

    async fn lsp_stop(&self, session_id: String) -> Result<(), WireError> {
        // The connection holding the session's stream lease is the only one
        // allowed to stop it; otherwise one desktop could kill another's server.
        let owner = self.session.lock().await.subscriber_id.clone();
        if !self.context.lsp.owns(&session_id, &owner) {
            return Err(WireError::InvalidArgument {
                message: format!("no LSP stream is open for session {session_id}"),
            });
        }
        self.host.lsp_stop(session_id).await.map_err(Into::into)
    }

    async fn pair(&self, token: String, name: String) -> Result<(), WireError> {
        if self.session.lock().await.authorized {
            return Ok(());
        }

        let _pairing = self.context.pairing.lock().await;
        if self.session.lock().await.authorized {
            return Ok(());
        }
        let fingerprint = self.session.lock().await.fingerprint;
        let config_dir = self.context.config_dir.clone();
        let static_token = self.context.auth_token.clone();
        let persisted = tokio::task::spawn_blocking(move || -> Result<bool, WireError> {
            let valid = auth::consume_pairing_token(&config_dir, &token, static_token.as_deref())
                .map_err(|error| WireError::Io {
                message: format!("consume pairing token: {error}"),
            })?;
            if !valid {
                return Ok(false);
            }
            auth::append_authorized(&config_dir, fingerprint, &name).map_err(|error| {
                WireError::Io {
                    message: format!("persist authorized client: {error}"),
                }
            })?;
            Ok(true)
        })
        .await
        .map_err(|error| WireError::Internal {
            message: format!("pairing task: {error}"),
        })??;
        if !persisted {
            return Err(unauthorized("invalid or expired pairing token"));
        }
        self.session.lock().await.authorized = true;
        Ok(())
    }
}

fn ensure_run(
    run_id: &str,
    run: &RunRecord,
    folder: &Path,
    expected: &RunKind,
) -> Result<(), WireError> {
    if run.folder == folder && &run.kind == expected {
        Ok(())
    } else {
        Err(conflicting_run(run_id))
    }
}

fn conflicting_run(run_id: &str) -> WireError {
    WireError::InvalidArgument {
        message: format!("run id is already bound to different metadata: {run_id}"),
    }
}

/// Desktop sections describe the machine a window runs on. A daemon that
/// accepted them would write settings nothing on its side ever reads, and the
/// desktop would silently stop owning its own terminal and window prefs.
fn reject_desktop_section(section: &str) -> Result<(), WireError> {
    if sworm_protocol::settings::is_desktop_section(section) {
        return Err(WireError::InvalidArgument {
            message: format!("{section} settings are desktop-only"),
        });
    }
    Ok(())
}

fn require_absolute(path: &str, field: &str) -> Result<(), WireError> {
    if Path::new(path).is_absolute() {
        Ok(())
    } else {
        Err(WireError::InvalidArgument {
            message: format!("remote {field} must be absolute: {path}"),
        })
    }
}

pub(crate) fn unauthorized(message: &str) -> WireError {
    WireError::Unauthorized {
        message: message.to_string(),
    }
}
