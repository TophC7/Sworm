use crate::{auth, events::HostEvents, pty_stream::RunFanout};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use sworm_core::Host;
use sworm_protocol::{
    rpc::{Reply, Request, Response, RunStatus, WireError, MAX_WHOLE_FILE_BYTES},
    session::SessionStartInfo,
};
use sworm_remote::Fingerprint;
use tokio::sync::{watch, Mutex, OwnedMutexGuard};

pub(crate) struct ServerContext {
    pub config_dir: PathBuf,
    pub auth_token: Option<String>,
    pub pairing: Mutex<()>,
    pub folders: parking_lot::Mutex<HashMap<PathBuf, usize>>,
    pub host_events: HostEvents,
    /// Table locks protect only lookup/publication. Gates serialize one ID's
    /// Host operation with its transport lease; weak entries disappear after
    /// the last waiter completes.
    runs: parking_lot::Mutex<RunRegistry>,
    /// Per-connection LSP streams, keyed by session id. A session's stream
    /// owns its server: closing it kills the process.
    pub lsp: crate::lsp_stream::LspStreams,
}

#[derive(Default)]
struct RunRegistry {
    gates: HashMap<String, Weak<Mutex<()>>>,
    leases: HashMap<String, Arc<RunFanout>>,
}

struct RunOperation<'a> {
    context: &'a ServerContext,
    run_id: String,
    gate: Arc<Mutex<()>>,
    guard: Option<OwnedMutexGuard<()>>,
}

impl Drop for RunOperation<'_> {
    fn drop(&mut self) {
        self.guard.take();
        let mut runs = self.context.runs.lock();
        // Keep the gate if another request already reserved it. Removing it
        // earlier could let a new start bypass that waiting request.
        if Arc::strong_count(&self.gate) == 1 {
            runs.gates.remove(&self.run_id);
        }
    }
}

impl ServerContext {
    pub(crate) fn new(
        config_dir: PathBuf,
        auth_token: Option<String>,
        host_events: HostEvents,
    ) -> Self {
        Self {
            config_dir,
            auth_token,
            pairing: Mutex::new(()),
            folders: parking_lot::Mutex::new(HashMap::new()),
            host_events,
            lsp: crate::lsp_stream::LspStreams::new(),
            runs: parking_lot::Mutex::new(RunRegistry::default()),
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

    async fn run_operation(&self, run_id: &str) -> RunOperation<'_> {
        let gate = {
            let mut runs = self.runs.lock();
            if let Some(gate) = runs.gates.get(run_id).and_then(Weak::upgrade) {
                gate
            } else {
                let gate = Arc::new(Mutex::new(()));
                runs.gates.insert(run_id.to_owned(), Arc::downgrade(&gate));
                gate
            }
        };
        let mut operation = RunOperation {
            context: self,
            run_id: run_id.to_owned(),
            gate,
            guard: None,
        };
        operation.guard = Some(Arc::clone(&operation.gate).lock_owned().await);
        operation
    }

    pub(crate) fn run_fanout(&self, run_id: &str) -> Option<Arc<RunFanout>> {
        self.runs.lock().leases.get(run_id).cloned()
    }

    async fn publish_run(&self, run_id: &str, live: bool) -> Result<(), WireError> {
        let removed = {
            let mut runs = self.runs.lock();
            if live {
                runs.leases
                    .entry(run_id.to_owned())
                    .or_insert_with(|| Arc::new(RunFanout::new()));
                None
            } else {
                runs.leases.remove(run_id)
            }
        };
        if let Some(fanout) = removed {
            tokio::task::spawn_blocking(move || fanout.cancel())
                .await
                .map_err(run_operation_join)?;
        }
        Ok(())
    }

    async fn invalidate_run(&self, run_id: &str) -> Result<(), WireError> {
        self.publish_run(run_id, false).await
    }
}

fn run_operation_join(error: tokio::task::JoinError) -> WireError {
    WireError::Internal {
        message: format!("run operation task failed: {error}"),
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

struct DispatchRuntime<'a> {
    host: &'a Arc<Host>,
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
    host: &Arc<Host>,
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

/// The auth and folder claim a `FileRead` stream needs, without the stat a
/// dispatched request would run: the stream's open validates the version.
pub(crate) async fn claim_file_read(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    session: &Mutex<Session>,
    project_path: &str,
) -> Result<(), WireError> {
    if !session.lock().await.authorized {
        return Err(unauthorized("client is not paired with this server"));
    }
    DispatchRuntime {
        host,
        context,
        session,
    }
    .claim(project_path, "project_path")
    .await
    .map(drop)
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

    /// Writes mirror the read ceiling: a file too large to read back is not
    /// one the daemon will store either.
    async fn file_write(
        &self,
        project_path: String,
        file_path: String,
        content: String,
        expected_version: Option<String>,
    ) -> Result<String, WireError> {
        if content.len() > MAX_WHOLE_FILE_BYTES {
            return Err(WireError::InvalidArgument {
                message: format!(
                    "File {file_path} exceeds the {MAX_WHOLE_FILE_BYTES}-byte write limit"
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
        self.claim(&folder_path, "folder_path").await?;
        // Once submitted, this owned operation finishes even if the RPC
        // stream disappears. Dropping a borrowed guard during Host's blocking
        // spawn would let Stop or a reused ID overtake the unfinished start.
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _operation = context.run_operation(&run_id).await;
            let info = host
                .session_start(
                    run_id.clone(),
                    folder_path,
                    provider_id,
                    resume_token,
                    cols,
                    rows,
                    None,
                    None,
                )
                .await
                .map_err(WireError::from)?;
            let state_host = Arc::clone(&host);
            let id = run_id.clone();
            let live = tokio::task::spawn_blocking(move || state_host.pty.run_state(&id).is_some())
                .await
                .map_err(run_operation_join)?;
            context.publish_run(&run_id, live).await?;
            Ok(info)
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn tasks_start(
        &self,
        run_id: String,
        folder_path: String,
        task_id: String,
        active_file_path: Option<String>,
        cols: u16,
        rows: u16,
        attach_only: bool,
    ) -> Result<(), WireError> {
        self.claim(&folder_path, "folder_path").await?;
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _operation = context.run_operation(&run_id).await;
            host.tasks_start(
                run_id.clone(),
                folder_path,
                task_id,
                active_file_path,
                cols,
                rows,
                None,
                None,
                attach_only,
            )
            .await
            .map_err(WireError::from)?;
            let state_host = Arc::clone(&host);
            let id = run_id.clone();
            let live = tokio::task::spawn_blocking(move || state_host.pty.run_state(&id).is_some())
                .await
                .map_err(run_operation_join)?;
            context.publish_run(&run_id, live).await
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn session_stop(&self, run_id: String) -> Result<(), WireError> {
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _operation = context.run_operation(&run_id).await;
            host.session_stop(run_id.clone())
                .await
                .map_err(WireError::from)?;
            context.invalidate_run(&run_id).await
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn tasks_stop(&self, run_id: String) -> Result<(), WireError> {
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _operation = context.run_operation(&run_id).await;
            host.tasks_stop(run_id.clone())
                .await
                .map_err(WireError::from)?;
            context.invalidate_run(&run_id).await
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn run_status(&self, run_id: String) -> Result<RunStatus, WireError> {
        let host = Arc::clone(self.host);
        tokio::task::spawn_blocking(move || host.run_status(&run_id))
            .await
            .map_err(run_operation_join)?
            .map_err(Into::into)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    #[tokio::test]
    async fn canceled_waiter_releases_its_run_gate() {
        let (events, _) = tokio::sync::broadcast::channel(2);
        let context = ServerContext::new(PathBuf::new(), None, events);
        let held = context.run_operation("same").await;
        let mut waiting = Box::pin(context.run_operation("same"));
        assert!(
            std::future::poll_fn(|cx| {
                std::task::Poll::Ready(waiting.as_mut().poll(cx).is_pending())
            })
            .await
        );
        drop(held);
        drop(waiting);
        assert!(context.runs.lock().gates.is_empty());
    }

    #[tokio::test]
    async fn queued_stop_invalidates_the_start_lease_before_id_reuse() {
        let (events, _) = tokio::sync::broadcast::channel(2);
        let context = Arc::new(ServerContext::new(PathBuf::new(), None, events));
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, resume) = tokio::sync::oneshot::channel();
        let starting = Arc::clone(&context);
        let start = tokio::spawn(async move {
            let _operation = starting.run_operation("same").await;
            entered.send(()).unwrap();
            resume.await.unwrap();
            starting.publish_run("same", true).await.unwrap();
        });
        started.await.unwrap();

        let stopping = Arc::clone(&context);
        let mut stop = Box::pin(async move {
            let _operation = stopping.run_operation("same").await;
            stopping.invalidate_run("same").await.unwrap();
        });
        // Poll once while Start holds the gate: Stop is now an actual queued
        // waiter, not merely an RPC sent on a different QUIC stream.
        assert!(
            std::future::poll_fn(|cx| {
                std::task::Poll::Ready(stop.as_mut().poll(cx).is_pending())
            })
            .await
        );
        release.send(()).unwrap();
        start.await.unwrap();
        stop.await;
        assert!(
            context.run_fanout("same").is_none(),
            "stop left a published lease"
        );
        assert!(
            context.runs.lock().gates.is_empty(),
            "run gate survived its last waiter"
        );

        let _replacement = context.run_operation("same").await;
        context.publish_run("same", true).await.unwrap();
        assert!(
            context.run_fanout("same").is_some(),
            "ID reuse lost its new lease"
        );
    }
}
