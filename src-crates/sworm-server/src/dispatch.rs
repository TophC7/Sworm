use crate::{auth, events::HostEvents, pty_stream::RunFanout};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use sworm_core::services::folders::resolve_folder;
use sworm_core::Host;
use sworm_protocol::{
    rpc::{Reply, Request, Response, RunStatus, WireError, MAX_WHOLE_FILE_BYTES},
    session::SessionStartInfo,
};
use sworm_remote::Fingerprint;
use tokio::sync::{
    watch, Mutex, OwnedMutexGuard, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock,
};

pub(crate) struct ServerContext {
    pub config_dir: PathBuf,
    pub auth_token: Option<String>,
    pub pairing: Mutex<()>,
    folders: parking_lot::Mutex<HashMap<PathBuf, FolderState>>,
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
struct FolderState {
    gate: Arc<RwLock<()>>,
    owners: usize,
}

// Shared guards cover resource publication; exclusive guards fence registration
// and teardown. The registry and session locks never cover Host work.
struct FolderOperation {
    context: Arc<ServerContext>,
    folder: PathBuf,
    gate: Arc<RwLock<()>>,
    read: Option<OwnedRwLockReadGuard<()>>,
    write: Option<OwnedRwLockWriteGuard<()>>,
}

impl FolderOperation {
    async fn read(&mut self) {
        self.read = Some(Arc::clone(&self.gate).read_owned().await);
    }

    async fn write(&mut self) {
        self.read.take();
        self.write = Some(Arc::clone(&self.gate).write_owned().await);
    }

    fn registration_needed(&self, host: &Host) -> bool {
        !host.settings_paths_watched(&self.folder)
    }

    async fn release(self, host: Arc<Host>, subscriber: String) -> Result<(), WireError> {
        tokio::task::spawn_blocking(move || {
            let last = {
                let mut folders = self.context.folders.lock();
                let state = folders.get_mut(&self.folder).expect("reserved folder");
                state.owners -= 1;
                state.owners == 0
            };
            host.file_watchers
                .release_subscriber_folder(&subscriber, &self.folder);
            if last {
                host.release_folder(&self.folder);
            }
            drop(self);
        })
        .await
        .map_err(folder_operation_join)
    }
}

impl Drop for FolderOperation {
    fn drop(&mut self) {
        self.read.take();
        self.write.take();
        let mut folders = self.context.folders.lock();
        if folders[&self.folder].owners == 0 && Arc::strong_count(&self.gate) == 2 {
            folders.remove(&self.folder);
        }
    }
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

    fn folder_operation(self: &Arc<Self>, folder: PathBuf) -> FolderOperation {
        let gate = Arc::clone(&self.folders.lock().entry(folder.clone()).or_default().gate);
        FolderOperation {
            context: Arc::clone(self),
            folder,
            gate,
            read: None,
            write: None,
        }
    }

    pub async fn release_folder(
        self: &Arc<Self>,
        host: &Arc<Host>,
        folder: PathBuf,
        subscriber: String,
    ) -> Result<(), WireError> {
        let mut operation = self.folder_operation(folder);
        operation.write().await;
        operation.release(Arc::clone(host), subscriber).await
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

fn folder_operation_join(error: tokio::task::JoinError) -> WireError {
    WireError::Internal {
        message: format!("folder operation task failed: {error}"),
    }
}

pub(crate) struct Session {
    pub fingerprint: Option<Fingerprint>,
    pub authorized: bool,
    pub subscriber_id: String,
    pub folders: HashSet<PathBuf>,
    pub folder_aliases: HashMap<PathBuf, PathBuf>,
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
/// Path-routed work claims a folder here; host-global work does not. Handwritten
/// exceptions handle browsing, explicit ownership, process metadata, streams,
/// and pairing. Missing implementations fail compilation at `dispatch`.
macro_rules! dispatch_operation {
    (#[route($route:ident)] FileWrite => $($rest:tt)*) => {};
    (#[route($route:ident)] FolderClaim => $($rest:tt)*) => {};
    (#[route($route:ident)] FolderRelease => $($rest:tt)*) => {};
    (#[route($route:ident)] FolderPathRoot => $($rest:tt)*) => {};
    (#[route($route:ident)] AppRuntimeInfo => $($rest:tt)*) => {};
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
            reject_native_section(&$input.section)?;
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
            let _folder = match $folder_path.as_deref() {
                Some(folder) => Some(self.claim(folder, "folder_path").await?),
                None => None,
            };
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
            let _folder = match $input.folder_path.as_deref() {
                Some(folder) => Some(self.claim(folder, "folder_path").await?),
                None => None,
            };
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
            let _folder = self.claim(&$input.folder_path, "folder_path").await?;
            self.host.$method($input).await.map_err(Into::into)
        }
    };
    (
        #[route(none)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $($argument: $argument_type),*) -> Result<$return_type, WireError> {
            self.host.$method($($argument),*).await.map_err(Into::into)
        }
    };
    (
        #[route($route:ident)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $($argument: $argument_type),*) -> Result<$return_type, WireError> {
            let _folder = self.claim(&$route, stringify!($route)).await?;
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
    async fn claim(&self, path: &str, field: &str) -> Result<FolderOperation, WireError> {
        require_absolute(path, field)?;
        let folder = self
            .session
            .lock()
            .await
            .folder_aliases
            .get(Path::new(path))
            .cloned()
            .unwrap_or_else(|| PathBuf::from(path));
        self.claim_folder(path, folder).await
    }

    async fn claim_folder(
        &self,
        path: &str,
        folder: PathBuf,
    ) -> Result<FolderOperation, WireError> {
        let mut operation = self.context.folder_operation(folder);
        operation.read().await;
        if operation.registration_needed(self.host) {
            operation.write().await;
        }
        {
            let mut session = self.session.lock().await;
            if session.folders.insert(operation.folder.clone()) {
                self.context
                    .folders
                    .lock()
                    .get_mut(&operation.folder)
                    .expect("reserved folder")
                    .owners += 1;
            }
            if Path::new(path) != operation.folder {
                session
                    .folder_aliases
                    .insert(PathBuf::from(path), operation.folder.clone());
            }
        }
        if operation.write.is_some() {
            let host = Arc::clone(self.host);
            operation = tokio::task::spawn_blocking(move || {
                if operation.registration_needed(&host) {
                    host.watch_settings_paths(Some(&operation.folder));
                }
                operation.read = operation.write.take().map(OwnedRwLockWriteGuard::downgrade);
                operation
            })
            .await
            .map_err(folder_operation_join)?;
        }
        Ok(operation)
    }

    async fn folder_claim(&self, folder_path: String) -> Result<(), WireError> {
        require_absolute(&folder_path, "folder_path")?;
        let folder = resolve_folder(&folder_path).map_err(WireError::from)?;
        self.claim_folder(&folder_path, folder).await.map(drop)
    }

    async fn folder_release(&self, folder_path: String) -> Result<(), WireError> {
        require_absolute(&folder_path, "folder_path")?;
        let recorded = self
            .session
            .lock()
            .await
            .folder_aliases
            .get(Path::new(&folder_path))
            .cloned();
        let folder = recorded.unwrap_or_else(|| {
            resolve_folder(&folder_path).unwrap_or_else(|_| PathBuf::from(&folder_path))
        });
        let mut operation = self.context.folder_operation(folder);
        operation.write().await;
        let subscriber = {
            let mut session = self.session.lock().await;
            if !session.folders.remove(&operation.folder) {
                return Ok(());
            }
            session
                .folder_aliases
                .retain(|_, folder| folder != &operation.folder);
            session.subscriber_id.clone()
        };
        operation.release(Arc::clone(self.host), subscriber).await
    }

    async fn folder_path_root(
        &self,
        path: String,
    ) -> Result<sworm_protocol::folder::PathRoot, WireError> {
        require_absolute(&path, "path")?;
        self.host.folder_path_root(path).await.map_err(Into::into)
    }

    async fn app_runtime_info(&self) -> Result<sworm_protocol::app::AppRuntimeInfo, WireError> {
        self.host
            .app_runtime_info(
                env!("CARGO_PKG_NAME").into(),
                env!("CARGO_PKG_VERSION").into(),
            )
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
        if content.len() > MAX_WHOLE_FILE_BYTES {
            return Err(WireError::InvalidArgument {
                message: format!(
                    "File {file_path} exceeds the {MAX_WHOLE_FILE_BYTES}-byte write limit"
                ),
            });
        }
        let _folder = self.claim(&project_path, "project_path").await?;
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
        let _folder = self.claim(&project_path, "project_path").await?;
        let subscriber = self.session.lock().await.subscriber_id.clone();
        self.host
            .files_watch_dirs(subscriber, project_path, dirs)
            .await
            .map_err(Into::into)
    }

    async fn git_watch(&self, project_path: String) -> Result<(), WireError> {
        let _folder = self.claim(&project_path, "project_path").await?;
        // The shared folder guard fences publication against release.
        self.host
            .git_watch(project_path, |_| true)
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
        // Once submitted, this owned operation finishes even if the RPC
        // stream disappears. Dropping a borrowed guard during Host's blocking
        // spawn would let Stop or a reused ID overtake the unfinished start.
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _folder = folder;
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
        let folder = self.claim(&folder_path, "folder_path").await?;
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _folder = folder;
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
        let _folder = self.claim(&folder_path, "folder_path").await?;
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
        let session = self.session.lock().await;
        let fingerprint = session
            .fingerprint
            .ok_or_else(|| WireError::InvalidArgument {
                message: "pairing is unavailable over web".to_owned(),
            })?;
        if session.authorized {
            return Ok(());
        }
        drop(session);

        let _pairing = self.context.pairing.lock().await;
        if self.session.lock().await.authorized {
            return Ok(());
        }
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

/// Remote users may set daemon presentation preferences; only connection
/// details remain native to the desktop.
fn reject_native_section(section: &str) -> Result<(), WireError> {
    if section == "remotes" {
        return Err(WireError::InvalidArgument {
            message: "remotes settings are desktop-only".to_owned(),
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
    async fn canceled_folder_waiter_releases_its_gate() {
        let (events, _) = tokio::sync::broadcast::channel(2);
        let context = Arc::new(ServerContext::new(PathBuf::new(), None, events));
        let mut held = context.folder_operation(PathBuf::from("/same"));
        held.read().await;
        let mut waiting = Box::pin(async {
            let mut operation = context.folder_operation(PathBuf::from("/same"));
            operation.write().await;
            operation
        });
        assert!(
            std::future::poll_fn(|cx| {
                std::task::Poll::Ready(waiting.as_mut().poll(cx).is_pending())
            })
            .await
        );
        drop(held);
        drop(waiting);
        assert!(context.folders.lock().is_empty());
    }
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
    fn paired_session(id: &str) -> Mutex<Session> {
        Mutex::new(Session {
            fingerprint: Some(Fingerprint([0; 32])),
            authorized: true,
            subscriber_id: id.into(),
            folders: HashSet::new(),
            folder_aliases: HashMap::new(),
            next_events: 0,
            events: None,
        })
    }

    // Run in a child so HOME/XDG never mutate the lib-test process shared by
    // other concurrently running unit tests.
    #[test]
    fn folder_ownership_and_concurrent_rearm() {
        if std::env::var_os("SWORM_DISPATCH_OWNERSHIP_CHILD").is_none() {
            let home = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("--exact")
                .arg("dispatch::tests::folder_ownership_and_concurrent_rearm")
                .arg("--nocapture")
                .env("SWORM_DISPATCH_OWNERSHIP_CHILD", "1")
                .env("HOME", home.path())
                .env("XDG_CONFIG_HOME", home.path().join("config"))
                .env("XDG_DATA_HOME", home.path().join("data"))
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .status()
                .unwrap();
            assert!(status.success(), "isolated folder ownership test failed");
            return;
        }
        let scratch = tempfile::tempdir().unwrap();
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let folder = scratch.path().join("project");
                std::fs::create_dir(&folder).unwrap();
                std::fs::create_dir(folder.join(".sworm")).unwrap();
                let alias = scratch.path().join("alias");
                std::os::unix::fs::symlink(&folder, &alias).unwrap();
                let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
                let host = Arc::new(
                    Host::new(
                        scratch.path().join("server.db"),
                        Arc::new(move |event| {
                            if let sworm_core::events::HostEvent::SettingsChanged(changed) = event {
                                let _ = sender.send(changed);
                            }
                            Ok(())
                        }),
                    )
                    .unwrap(),
                );
                let (broadcast, _) = tokio::sync::broadcast::channel(4);
                let context = Arc::new(ServerContext::new(
                    scratch.path().join("config"),
                    None,
                    broadcast,
                ));
                let a = Arc::new(paired_session("a"));
                let b = Arc::new(paired_session("b"));
                let path = folder.to_string_lossy().into_owned();
                let alias_path = alias.to_string_lossy().into_owned();

                let root = handle(
                    &host,
                    &context,
                    &a,
                    Request::FolderPathRoot { path: path.clone() },
                )
                .await
                .unwrap();
                assert!(matches!(root, Reply::FolderPathRoot(_)));
                assert!(
                    context.folders.lock().is_empty(),
                    "browsing claimed a folder"
                );
                assert!(matches!(
                    handle(
                        &host,
                        &context,
                        &a,
                        Request::FolderPathRoot { path: ".".into() }
                    )
                    .await,
                    Err(WireError::InvalidArgument { .. })
                ));
                assert!(context.folders.lock().is_empty());

                let claim = |session: &Arc<Mutex<Session>>, path: &str| {
                    let host = Arc::clone(&host);
                    let context = Arc::clone(&context);
                    let session = Arc::clone(session);
                    let path = path.to_owned();
                    async move {
                        handle(
                            &host,
                            &context,
                            &session,
                            Request::FolderClaim { folder_path: path },
                        )
                        .await
                        .unwrap();
                    }
                };
                let release = |session: &Arc<Mutex<Session>>, path: &str| {
                    let host = Arc::clone(&host);
                    let context = Arc::clone(&context);
                    let session = Arc::clone(session);
                    let path = path.to_owned();
                    async move {
                        handle(
                            &host,
                            &context,
                            &session,
                            Request::FolderRelease { folder_path: path },
                        )
                        .await
                        .unwrap();
                    }
                };
                claim(&a, &alias_path).await;
                claim(&a, &path).await;
                assert_eq!(a.lock().await.folders.len(), 1);
                assert_eq!(
                    context
                        .folders
                        .lock()
                        .get(&folder)
                        .map(|state| state.owners),
                    Some(1)
                );
                release(&b, &path).await;
                assert_eq!(
                    context
                        .folders
                        .lock()
                        .get(&folder)
                        .map(|state| state.owners),
                    Some(1)
                );
                claim(&b, &path).await;
                assert_eq!(
                    context
                        .folders
                        .lock()
                        .get(&folder)
                        .map(|state| state.owners),
                    Some(2)
                );
                std::fs::remove_file(&alias).unwrap();
                release(&a, &alias_path).await;
                assert!(
                    a.lock().await.folders.is_empty(),
                    "removing a claimed alias must not strand canonical ownership"
                );
                std::os::unix::fs::symlink(&folder, &alias).unwrap();
                release(&a, &path).await;
                assert!(a.lock().await.folders.is_empty());
                assert_eq!(
                    context
                        .folders
                        .lock()
                        .get(&folder)
                        .map(|state| state.owners),
                    Some(1)
                );
                for _ in 0..2 {
                    let resolved = handle(
                        &host,
                        &context,
                        &a,
                        Request::FolderResolve { path: path.clone() },
                    )
                    .await
                    .unwrap();
                    assert!(matches!(resolved, Reply::FolderResolve(_)));
                }
                assert_eq!(a.lock().await.folders.len(), 1);
                assert_eq!(
                    context
                        .folders
                        .lock()
                        .get(&folder)
                        .map(|state| state.owners),
                    Some(2)
                );
                release(&a, &path).await;
                assert_eq!(
                    context
                        .folders
                        .lock()
                        .get(&folder)
                        .map(|state| state.owners),
                    Some(1)
                );

                for iteration in 0..8 {
                    // Either order must retain B's replacement owner's watcher.
                    let start = Arc::new(tokio::sync::Barrier::new(3));
                    let releasing = tokio::spawn({
                        let host = Arc::clone(&host);
                        let context = Arc::clone(&context);
                        let b = Arc::clone(&b);
                        let path = path.clone();
                        let start = Arc::clone(&start);
                        async move {
                            start.wait().await;
                            handle(
                                &host,
                                &context,
                                &b,
                                Request::FolderRelease { folder_path: path },
                            )
                            .await
                            .unwrap();
                        }
                    });
                    let claiming = tokio::spawn({
                        let host = Arc::clone(&host);
                        let context = Arc::clone(&context);
                        let a = Arc::clone(&a);
                        let path = path.clone();
                        let start = Arc::clone(&start);
                        async move {
                            start.wait().await;
                            handle(
                                &host,
                                &context,
                                &a,
                                Request::FolderClaim { folder_path: path },
                            )
                            .await
                            .unwrap();
                        }
                    });
                    start.wait().await;
                    releasing.await.unwrap();
                    claiming.await.unwrap();
                    assert!(b.lock().await.folders.is_empty());
                    assert!(a.lock().await.folders.contains(&folder));
                    assert_eq!(
                        context
                            .folders
                            .lock()
                            .get(&folder)
                            .map(|state| state.owners),
                        Some(1)
                    );
                    while events.try_recv().is_ok() {}
                    let settings = folder.join(".sworm/settings.jsonc");
                    let mut observed = false;
                    for attempt in 0..5 {
                        std::fs::write(
                            &settings,
                            format!("{{\"marker\":{}}}", iteration * 5 + attempt),
                        )
                        .unwrap();
                        if let Ok(Some(changed)) = tokio::time::timeout(
                            std::time::Duration::from_millis(400),
                            events.recv(),
                        )
                        .await
                        {
                            if changed.folder_path.as_deref() == Some(path.as_str()) {
                                observed = true;
                                break;
                            }
                        }
                    }
                    assert!(
                        observed,
                        "last release removed the new owner's settings watcher"
                    );
                    claim(&b, &path).await;
                    release(&a, &path).await;
                }

                let runtime = DispatchRuntime {
                    host: &host,
                    context: &context,
                    session: &b,
                };
                let publication = runtime.claim(&path, "folder_path").await.unwrap();
                // Already-owned operations share the fence; a queued release
                // waits for publication without retaining the session mutex.
                drop(
                    tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        runtime.claim(&path, "folder_path"),
                    )
                    .await
                    .unwrap()
                    .unwrap(),
                );
                let mut releasing = Box::pin(release(&b, &path));
                assert!(
                    std::future::poll_fn(|cx| {
                        std::task::Poll::Ready(releasing.as_mut().poll(cx).is_pending())
                    })
                    .await
                );
                let other = scratch.path().join("other");
                std::fs::create_dir(&other).unwrap();
                let other_path = other.to_string_lossy().into_owned();
                tokio::time::timeout(std::time::Duration::from_secs(2), claim(&b, &other_path))
                    .await
                    .unwrap();
                host.tasks_list(path.clone()).await.unwrap();
                drop(publication);
                tokio::time::timeout(std::time::Duration::from_secs(2), releasing)
                    .await
                    .unwrap();
                assert!(!b.lock().await.folders.contains(&folder));
                assert!(!context.folders.lock().contains_key(&folder));
                release(&b, &other_path).await;

                assert!(std::process::Command::new("git")
                    .args(["init", "--quiet"])
                    .arg(&folder)
                    .status()
                    .unwrap()
                    .success());
                for _ in 0..8 {
                    claim(&b, &path).await;
                    tokio::time::timeout(std::time::Duration::from_secs(5), async {
                        let (watch, ()) = tokio::join!(
                            handle(
                                &host,
                                &context,
                                &b,
                                Request::GitWatch {
                                    project_path: path.clone()
                                }
                            ),
                            release(&b, &path)
                        );
                        watch.unwrap();
                    })
                    .await
                    .unwrap();
                    release(&b, &path).await;
                }

                // A failed first watch must retry when the directory appears.
                let delayed = scratch.path().join("delayed");
                let delayed_path = delayed.to_string_lossy().into_owned();
                drop(runtime.claim(&delayed_path, "folder_path").await.unwrap());
                std::fs::create_dir_all(delayed.join(".sworm")).unwrap();
                claim(&b, &delayed_path).await;
                while events.try_recv().is_ok() {}
                std::fs::write(delayed.join(".sworm/settings.jsonc"), "{}").unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        let changed = events.recv().await.unwrap();
                        if changed.folder_path.as_deref() == Some(delayed_path.as_str()) {
                            break;
                        }
                    }
                })
                .await
                .unwrap();
                std::fs::remove_dir_all(delayed.join(".sworm")).unwrap();
                std::fs::create_dir(delayed.join(".sworm")).unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    while host.settings_paths_watched(&delayed) {
                        events.recv().await.unwrap();
                    }
                    // Drain the directory replacement before observing a
                    // subsequent file write through the replacement watch.
                    while tokio::time::timeout(std::time::Duration::from_millis(50), events.recv())
                        .await
                        .is_ok()
                    {}
                })
                .await
                .unwrap();
                claim(&b, &delayed_path).await;
                std::fs::write(delayed.join(".sworm/settings.jsonc"), "{}").unwrap();
                tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    loop {
                        let changed = events.recv().await.unwrap();
                        if changed.folder_path.as_deref() == Some(delayed_path.as_str()) {
                            break;
                        }
                    }
                })
                .await
                .unwrap();
                release(&b, &delayed_path).await;
                claim(&b, &path).await;

                std::fs::remove_dir_all(&folder).unwrap();
                release(&b, &path).await;
                assert!(b.lock().await.folders.is_empty());
                assert!(context.folders.lock().is_empty());
            });
    }
}
