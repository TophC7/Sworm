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

    async fn files_read_dir(
        &self,
        project_path: String,
        dir_path: String,
        show_hidden: bool,
    ) -> Result<Vec<sworm_protocol::files::DirEntry>, WireError> {
        self.claim(&project_path, "project_path").await?;
        self.host
            .files_read_dir(project_path, dir_path, show_hidden)
            .await
            .map_err(Into::into)
    }

    async fn file_read(
        &self,
        project_path: String,
        file_path: String,
    ) -> Result<String, WireError> {
        self.claim(&project_path, "project_path").await?;
        self.host
            .file_read_limited(project_path, file_path, MAX_REMOTE_FILE_BYTES)
            .await
            .map_err(Into::into)
    }

    async fn git_get_summary(
        &self,
        path: String,
    ) -> Result<sworm_protocol::git::GitSummary, WireError> {
        self.claim(&path, "path").await?;
        self.host.git_get_summary(path).await.map_err(Into::into)
    }

    async fn folder_resolve(
        &self,
        path: String,
    ) -> Result<sworm_protocol::folder::FolderInfo, WireError> {
        self.claim(&path, "path").await?;
        self.host.folder_resolve(path).await.map_err(Into::into)
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

    async fn tasks_list(
        &self,
        folder_path: String,
    ) -> Result<Vec<sworm_protocol::task::TaskDefinition>, WireError> {
        self.claim(&folder_path, "folder_path").await?;
        self.host.tasks_list(folder_path).await.map_err(Into::into)
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
