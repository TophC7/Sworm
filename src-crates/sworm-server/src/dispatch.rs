use crate::{
    auth,
    events::HostEvents,
    pty_stream::RunFanout,
    workbenches::{self, Attach, ControlLease, Terminal},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};
use sworm_core::services::folders::resolve_folder;
use sworm_core::{errors::ApiError, Host};
use sworm_protocol::{
    rpc::{Reply, Request, Response, RunStatus, WireError, MAX_WHOLE_FILE_BYTES},
    session::SessionStartInfo,
};
use sworm_remote::Fingerprint;
use tokio::sync::{
    watch, Mutex, OwnedMutexGuard, OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock,
};
use tokio_util::task::TaskTracker;

pub(crate) struct ServerContext {
    pub config_dir: PathBuf,
    pub authorized_keys_file: Option<PathBuf>,
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
    /// Durable workbench registry and its per-workbench transition slots.
    pub(crate) workbenches: workbenches::Workbenches,
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
        authorized_keys_file: Option<PathBuf>,
        host_events: HostEvents,
    ) -> Self {
        Self {
            config_dir,
            authorized_keys_file,
            pairing: Mutex::new(()),
            folders: parking_lot::Mutex::new(HashMap::new()),
            host_events,
            lsp: crate::lsp_stream::LspStreams::new(),
            runs: parking_lot::Mutex::new(RunRegistry::default()),
            workbenches: workbenches::Workbenches::new(),
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

    async fn publish_if_live(&self, host: &Arc<Host>, run_id: &str) -> Result<(), WireError> {
        let host = Arc::clone(host);
        let id = run_id.to_owned();
        let live = tokio::task::spawn_blocking(move || host.pty.run_state(&id).is_some())
            .await
            .map_err(ApiError::from)?;
        self.publish_run(run_id, live).await
    }

    async fn invalidate_run(&self, run_id: &str) -> Result<(), WireError> {
        self.publish_run(run_id, false).await
    }

    /// After an owner stop, retire the transport lease of every run that is
    /// gone. Runs whose stop failed stay published for the retry.
    pub(crate) async fn retire_stopped_runs(
        &self,
        host: &Arc<Host>,
        run_ids: Vec<String>,
    ) -> Result<(), WireError> {
        for run_id in run_ids {
            let _operation = self.run_operation(&run_id).await;
            let state_host = Arc::clone(host);
            let id = run_id.clone();
            let live = tokio::task::spawn_blocking(move || state_host.pty.run_state(&id).is_some())
                .await
                .map_err(run_operation_join)?;
            if !live {
                self.invalidate_run(&run_id).await?;
            }
        }
        Ok(())
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

/// Scope of a request. Only an admitted workbench control supplies its owner;
/// an unscoped QUIC request cannot choose one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SessionScope {
    Quic,
    /// `owner` is the record's run-owner incarnation, resolved at attach: the
    /// Host tombstones closed owners, so a recreated id needs a fresh one.
    Workbench {
        id: String,
        owner: String,
    },
}

impl SessionScope {
    /// Stable PTY owner. LSP ownership stays connection-scoped (`subscriber_id`).
    fn run_owner(&self) -> Option<&str> {
        match self {
            Self::Workbench { owner, .. } => Some(owner),
            Self::Quic => None,
        }
    }
}

pub(crate) struct Lease {
    pub control: Arc<ControlLease>,
    token: String,
    attachment_id: String,
    pub owner: String,
}

/// One retained attempt per workbench on this authenticated connection.
/// Supersession wakes recovery immediately; the shared gate fences publication.
struct AttachAttempt {
    attachment_id: String,
    gate: Arc<Mutex<()>>,
    finished: watch::Sender<bool>,
}

struct FinishAttach(Arc<AttachAttempt>);

impl Drop for FinishAttach {
    fn drop(&mut self) {
        self.0.finished.send_replace(true);
    }
}

const MAX_CANCELLED_ATTACHMENTS: usize = 64;

fn recovery_capacity() -> WireError {
    WireError::InvalidArgument {
        message: "Attach recovery capacity exhausted; reconnect before attaching".into(),
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
    pub scope: SessionScope,
    pub leases: Arc<Mutex<HashMap<String, Lease>>>,
    attach_attempts: HashMap<String, Arc<AttachAttempt>>,
    /// Unknown recovery cancels that exact late arrival. Never evict these
    /// identities: at capacity, new attach/unknown recovery fails closed.
    cancelled_attachments: HashMap<String, HashSet<String>>,
    cancelled_attachment_count: usize,
    pub lease_watchers: TaskTracker,
}

impl Session {
    pub(crate) fn new(
        fingerprint: Option<Fingerprint>,
        authorized: bool,
        scope: SessionScope,
    ) -> Self {
        Self {
            fingerprint,
            authorized,
            subscriber_id: uuid::Uuid::new_v4().to_string(),
            folders: HashSet::new(),
            folder_aliases: HashMap::new(),
            next_events: 0,
            events: None,
            scope,
            leases: Arc::new(Mutex::new(HashMap::new())),
            attach_attempts: HashMap::new(),
            cancelled_attachments: HashMap::new(),
            cancelled_attachment_count: 0,
            lease_watchers: TaskTracker::new(),
        }
    }
}

struct DispatchRuntime<'a> {
    host: &'a Arc<Host>,
    context: &'a Arc<ServerContext>,
    session: &'a Mutex<Session>,
    scope: SessionScope,
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
    (#[route($route:ident)] LspStart => $($rest:tt)*) => {};
    // The manifest and registered snapshots belong to the workbench registry.
    (#[route($route:ident)] AppStateGet => $($rest:tt)*) => {};
    (#[route($route:ident)] AppStatePut => $($rest:tt)*) => {};
    (#[route($route:ident)] AppStateDelete => $($rest:tt)*) => {};
    (#[route(connection)] $($rest:tt)*) => {};
    (#[route(run)] $($rest:tt)*) => {};
    (
        #[route(server)]
        SettingsPatchGlobalSection => $method:ident(
            $input:ident: $input_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $input: $input_type) -> Result<$return_type, WireError> {
            reject_native_section(&$input.section)?;
            self.on_host(move |host| host.$method($input)).await
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
            self.on_host(move |host| host.$method($($argument),*)).await
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
            self.on_host(move |host| host.$method($folder_path)).await
        }
    };
    (
        #[route(none)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        async fn $method(&self, $($argument: $argument_type),*) -> Result<$return_type, WireError> {
            self.on_host(move |host| host.$method($($argument),*)).await
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
            self.on_host(move |host| host.$method($($argument),*)).await
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
    let scope = session.lock().await.scope.clone();
    handle_scoped(host, context, session, scope, request).await
}

pub(crate) async fn handle_scoped(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    session: &Mutex<Session>,
    scope: SessionScope,
    request: Request,
) -> Response {
    {
        let session = session.lock().await;
        if !matches!(&request, Request::Pair { .. }) && !session.authorized {
            return Err(unauthorized("client is not paired with this server"));
        }
    }
    DispatchRuntime {
        host,
        context,
        session,
        scope,
    }
    .dispatch(request)
    .await
}

/// Workbenches act only on their own live runs; runs absent from memory
/// keep the existing UUID-only, read-only archive semantics. Caller holds the
/// run gate.
async fn check_run_owner(
    host: &Arc<Host>,
    owner: Option<String>,
    run_id: &str,
) -> Result<(), WireError> {
    let Some(owner) = owner else {
        return Ok(());
    };
    let host = Arc::clone(host);
    let id = run_id.to_owned();
    tokio::task::spawn_blocking(move || {
        if host.pty.run_state(&id).is_none() {
            return Ok(());
        }
        host.pty
            .ensure_owner(&id, Some(&owner))
            .map_err(|message| WireError::Unauthorized { message })
    })
    .await
    .map_err(run_operation_join)?
}

/// Ownership check for status and stream access under the run gate.
pub(crate) async fn authorize_run(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    session: &Mutex<Session>,
    run_id: &str,
) -> Result<(), WireError> {
    let owner = session.lock().await.scope.run_owner().map(str::to_owned);
    if owner.is_none() {
        return Ok(());
    }
    let _operation = context.run_operation(run_id).await;
    check_run_owner(host, owner, run_id).await
}

/// The auth and folder claim a `FileRead` stream needs, without the stat a
/// dispatched request would run: the stream's open validates the version.
pub(crate) async fn claim_file_read(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    session: &Mutex<Session>,
    project_path: &str,
) -> Result<(), WireError> {
    let scope = {
        let session = session.lock().await;
        if !session.authorized {
            return Err(unauthorized("client is not paired with this server"));
        }
        session.scope.clone()
    };
    DispatchRuntime {
        host,
        context,
        session,
        scope,
    }
    .claim(project_path, "project_path")
    .await
    .map(drop)
}

impl DispatchRuntime<'_> {
    async fn on_host<T: Send + 'static>(
        &self,
        op: impl FnOnce(&Arc<Host>) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, WireError> {
        let host = Arc::clone(self.host);
        Ok(tokio::task::spawn_blocking(move || op(&host))
            .await
            .map_err(ApiError::from)??)
    }

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
        self.on_host(move |host| host.folder_path_root(path)).await
    }

    async fn app_runtime_info(&self) -> Result<sworm_protocol::app::AppRuntimeInfo, WireError> {
        self.on_host(move |host| {
            host.app_runtime_info(
                env!("CARGO_PKG_NAME").into(),
                env!("CARGO_PKG_VERSION").into(),
            )
        })
        .await
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
        self.on_host(move |host| {
            host.file_write(project_path, file_path, content, expected_version)
        })
        .await
    }

    /// Browsing is not ownership: the browser walks directories the
    /// desktop never opens, so this claims nothing.
    async fn folder_list_entries(
        &self,
        path: String,
        show_hidden: bool,
    ) -> Result<Vec<sworm_protocol::folder::FolderEntry>, WireError> {
        require_absolute(&path, "path")?;
        self.on_host(move |host| host.folder_list_entries(path, show_hidden))
            .await
    }

    async fn files_watch_dirs(
        &self,
        project_path: String,
        dirs: Vec<String>,
    ) -> Result<(), WireError> {
        let _folder = self.claim(&project_path, "project_path").await?;
        let subscriber = self.session.lock().await.subscriber_id.clone();
        self.on_host(move |host| host.files_watch_dirs(subscriber, project_path, dirs))
            .await
    }

    async fn git_watch(&self, project_path: String) -> Result<(), WireError> {
        let _folder = self.claim(&project_path, "project_path").await?;
        // The shared folder guard fences publication against release.
        self.on_host(move |host| host.git_watch(project_path, |_| true))
            .await
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
        let owner = self.run_owner();
        // Once submitted, this owned operation finishes even if the RPC
        // stream disappears. Dropping a borrowed guard during Host's blocking
        // spawn would let Stop or a reused ID overtake the unfinished start.
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _folder = folder;
            let _operation = context.run_operation(&run_id).await;
            let id = run_id.clone();
            let start_host = Arc::clone(&host);
            let info = tokio::task::spawn_blocking(move || {
                start_host.session_start(
                    id,
                    folder_path,
                    provider_id,
                    resume_token,
                    cols,
                    rows,
                    None,
                    owner,
                )
            })
            .await
            .map_err(ApiError::from)??;
            context.publish_if_live(&host, &run_id).await?;
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
        let owner = self.run_owner();
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _folder = folder;
            let _operation = context.run_operation(&run_id).await;
            let id = run_id.clone();
            let start_host = Arc::clone(&host);
            tokio::task::spawn_blocking(move || {
                start_host.tasks_start(
                    id,
                    folder_path,
                    task_id,
                    active_file_path,
                    cols,
                    rows,
                    None,
                    owner,
                    attach_only,
                )
            })
            .await
            .map_err(ApiError::from)??;
            context.publish_if_live(&host, &run_id).await
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn session_stop(&self, run_id: String) -> Result<(), WireError> {
        let owner = self.run_owner();
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _operation = context.run_operation(&run_id).await;
            check_run_owner(&host, owner, &run_id).await?;
            let id = run_id.clone();
            tokio::task::spawn_blocking(move || host.session_stop(id))
                .await
                .map_err(ApiError::from)??;
            context.invalidate_run(&run_id).await
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn tasks_stop(&self, run_id: String) -> Result<(), WireError> {
        let owner = self.run_owner();
        let host = Arc::clone(self.host);
        let context = Arc::clone(self.context);
        tokio::spawn(async move {
            let _operation = context.run_operation(&run_id).await;
            check_run_owner(&host, owner, &run_id).await?;
            let id = run_id.clone();
            tokio::task::spawn_blocking(move || host.tasks_stop(id))
                .await
                .map_err(ApiError::from)??;
            context.invalidate_run(&run_id).await
        })
        .await
        .map_err(run_operation_join)?
    }

    async fn run_status(&self, run_id: String) -> Result<RunStatus, WireError> {
        let _operation = self.context.run_operation(&run_id).await;
        check_run_owner(self.host, self.run_owner(), &run_id).await?;
        self.on_host(move |host| host.run_status(&run_id)).await
    }
    fn run_owner(&self) -> Option<String> {
        self.scope.run_owner().map(str::to_owned)
    }

    /// Registry snapshots belong to their workbench; unscoped QUIC keeps
    /// arbitrary keys except snapshots of registered workbenches.
    async fn app_state_scope(&self, key: &str, delete: bool) -> Result<Option<String>, WireError> {
        if key == workbenches::MANIFEST_KEY {
            return Err(unauthorized("The workbench manifest is reserved"));
        }
        match &self.scope {
            SessionScope::Workbench { id, .. } => {
                if let Some(target) = workbenches::snapshot_id(key) {
                    if target != id.as_str() {
                        return Err(unauthorized(
                            "Workbench snapshot belongs to another workbench",
                        ));
                    }
                    if delete {
                        return Err(unauthorized(
                            "Close Workbench is the only way to delete a workbench snapshot",
                        ));
                    }
                    return Ok(Some(id.clone()));
                }
                // Legacy unscoped keys carry an absolute folder path.
                if let Some(rest) = key.strip_prefix("branchesView:") {
                    if !rest.starts_with('/') && rest.split(':').next() != Some(id.as_str()) {
                        return Err(unauthorized("Preferences belong to another workbench"));
                    }
                }
                Ok(None)
            }
            SessionScope::Quic => {
                if let Some(target) = workbenches::snapshot_id(key) {
                    if workbenches::is_registered(self.host, target).await? {
                        return Err(unauthorized("Workbench snapshots are server-managed"));
                    }
                }
                Ok(None)
            }
        }
    }

    async fn app_state_get(&self, key: String) -> Result<Option<String>, WireError> {
        self.app_state_scope(&key, false).await?;
        self.on_host(move |host| host.app_state_get(key)).await
    }

    async fn app_state_put(&self, key: String, value_json: String) -> Result<(), WireError> {
        match self.app_state_scope(&key, false).await? {
            Some(id) => workbenches::put_snapshot(self.host, id, value_json).await,
            None => {
                self.on_host(move |host| host.app_state_put(key, value_json))
                    .await
            }
        }
    }

    async fn app_state_delete(&self, key: String) -> Result<(), WireError> {
        self.app_state_scope(&key, true).await?;
        self.on_host(move |host| host.app_state_delete(key)).await
    }

    async fn workbench_list(&self) -> Result<Vec<sworm_protocol::rpc::WorkbenchInfo>, WireError> {
        let yours: HashSet<String> = match &self.scope {
            SessionScope::Workbench { id, .. } => [id.clone()].into(),
            SessionScope::Quic => {
                let leases = self.session.lock().await.leases.clone();
                let held = leases.lock().await;
                held.iter()
                    .filter(|(_, lease)| lease.control.admitted())
                    .map(|(id, _)| id.clone())
                    .collect()
            }
        };
        workbenches::list(self.host, self.context, &yours).await
    }

    async fn workbench_close(&self, id: String) -> Result<(), WireError> {
        let caller = match &self.scope {
            SessionScope::Workbench { id, .. } => Some(id.as_str()),
            SessionScope::Quic => None,
        };
        workbenches::close(self.host, self.context, id, caller).await
    }

    async fn workbench_attach(
        &self,
        id: String,
        attachment_id: String,
        mode: sworm_protocol::rpc::AttachMode,
        client: String,
    ) -> Result<sworm_protocol::rpc::WorkbenchAttached, WireError> {
        if !matches!(self.scope, SessionScope::Quic) {
            return Err(WireError::InvalidArgument {
                message: "Workbench attach requires QUIC".into(),
            });
        }
        let (attempt, leases, watcher) = {
            let mut session = self.session.lock().await;
            if session
                .cancelled_attachments
                .get(&id)
                .is_some_and(|cancelled| cancelled.contains(&attachment_id))
            {
                return Err(WireError::NotController { workbench: id });
            }
            if session.cancelled_attachment_count == MAX_CANCELLED_ATTACHMENTS {
                return Err(recovery_capacity());
            }
            let previous = session.attach_attempts.get(&id);
            if previous.is_some_and(|previous| previous.attachment_id == attachment_id) {
                return Err(WireError::InvalidArgument {
                    message: "Attachment id already used; recover the existing attempt".into(),
                });
            }
            let gate = previous.map_or_else(
                || Arc::new(Mutex::new(())),
                |previous| {
                    previous.finished.send_replace(true);
                    Arc::clone(&previous.gate)
                },
            );
            let attempt = Arc::new(AttachAttempt {
                attachment_id: attachment_id.clone(),
                gate,
                finished: watch::channel(false).0,
            });
            session
                .attach_attempts
                .insert(id.clone(), Arc::clone(&attempt));
            (
                attempt,
                session.leases.clone(),
                session.lease_watchers.clone(),
            )
        };
        let _finish = FinishAttach(Arc::clone(&attempt));
        let _gate = attempt.gate.lock().await;
        if !self.current_attempt(&id, &attempt).await {
            return Err(WireError::NotController { workbench: id });
        }
        let outcome =
            workbenches::attach(self.host, self.context, id.clone(), mode, client).await?;
        use sworm_protocol::rpc::WorkbenchAttached;
        match outcome {
            Attach::Busy { client, snapshot } => Ok(WorkbenchAttached::Busy { client, snapshot }),
            Attach::Revoked { client, snapshot } => {
                Ok(WorkbenchAttached::Revoked { client, snapshot })
            }
            Attach::Ready {
                token,
                lease,
                owner,
                snapshot,
            } => {
                let session = self.session.lock().await;
                let current = session
                    .attach_attempts
                    .get(&id)
                    .is_some_and(|current| Arc::ptr_eq(current, &attempt));
                if current {
                    let previous = leases.lock().await.insert(
                        id.clone(),
                        Lease {
                            control: Arc::clone(&lease),
                            token: token.clone(),
                            attachment_id: attachment_id.clone(),
                            owner,
                        },
                    );
                    if let Some(previous) = previous {
                        previous.control.retire(Terminal::Disconnected);
                    }
                } else {
                    lease.retire(Terminal::Disconnected);
                }
                drop(session);
                let host = Arc::clone(self.host);
                let context = Arc::clone(self.context);
                let detach_token = token.clone();
                let detach_id = id.clone();
                // Teardown awaits every watcher so detach commits before shutdown.
                watcher.spawn(async move {
                    lease.retired().await;
                    lease.drained().await;
                    lease.complete(Ok(()));
                    let mut leases = leases.lock().await;
                    if leases.get(&detach_id).is_some_and(|current| {
                        Arc::ptr_eq(&current.control, &lease) && current.token == detach_token
                    }) {
                        leases.remove(&detach_id);
                    }
                    drop(leases);
                    workbenches::detach(&host, &context, detach_id, detach_token, &lease).await;
                });
                if !current {
                    return Err(WireError::NotController { workbench: id });
                }
                Ok(WorkbenchAttached::Ready {
                    attachment_id,
                    controller_token: token,
                    snapshot,
                })
            }
        }
    }

    async fn current_attempt(&self, id: &str, attempt: &Arc<AttachAttempt>) -> bool {
        self.session
            .lock()
            .await
            .attach_attempts
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, attempt))
    }

    async fn workbench_recover(
        &self,
        id: String,
        attachment_id: String,
    ) -> Result<Option<sworm_protocol::rpc::WorkbenchAttached>, WireError> {
        if !matches!(self.scope, SessionScope::Quic) {
            return Err(WireError::InvalidArgument {
                message: "Workbench recovery requires QUIC".into(),
            });
        }
        let (attempt, leases) = {
            let mut session = self.session.lock().await;
            let Some(attempt) = session
                .attach_attempts
                .get(&id)
                .filter(|attempt| attempt.attachment_id == attachment_id)
                .cloned()
            else {
                if session
                    .cancelled_attachments
                    .get(&id)
                    .is_some_and(|cancelled| cancelled.contains(&attachment_id))
                {
                    return Ok(None);
                }
                if session.cancelled_attachment_count == MAX_CANCELLED_ATTACHMENTS {
                    return Err(recovery_capacity());
                }
                session
                    .cancelled_attachments
                    .entry(id)
                    .or_default()
                    .insert(attachment_id);
                session.cancelled_attachment_count += 1;
                return Ok(None);
            };
            (attempt, session.leases.clone())
        };
        let mut finished = attempt.finished.subscribe();
        let _ = finished.wait_for(|done| *done).await;
        if !self.current_attempt(&id, &attempt).await {
            return Ok(None);
        }
        let recovered = {
            let leases = leases.lock().await;
            leases
                .get(&id)
                .filter(|lease| lease.attachment_id == attachment_id && lease.control.admitted())
                .map(|lease| (lease.token.clone(), Arc::clone(&lease.control)))
        };
        let Some((controller_token, control)) = recovered else {
            return Ok(None);
        };
        let snapshot = workbenches::snapshot(self.host, &id).await;
        // Close/takeover can retire the control while the snapshot is read.
        if !self.current_attempt(&id, &attempt).await || !control.admitted() {
            return Ok(None);
        }
        Ok(Some(sworm_protocol::rpc::WorkbenchAttached::Ready {
            attachment_id,
            controller_token,
            snapshot: snapshot?,
        }))
    }

    async fn workbench_detach(&self, id: String, attachment_id: String) -> Result<(), WireError> {
        let leases = self.session.lock().await.leases.clone();
        let lease = leases
            .lock()
            .await
            .get(&id)
            .filter(|lease| lease.attachment_id == attachment_id)
            .map(|lease| Arc::clone(&lease.control));
        if let Some(control) = lease {
            control.retire(Terminal::Disconnected);
        }
        Ok(())
    }

    async fn workbench_save(&self, id: String, snapshot: String) -> Result<(), WireError> {
        match &self.scope {
            SessionScope::Workbench { id: own, .. } if *own == id => {
                workbenches::put_snapshot(self.host, id, snapshot).await
            }
            SessionScope::Quic => {
                let leases = self.session.lock().await.leases.clone();
                let tracking = leases
                    .lock()
                    .await
                    .get(&id)
                    .and_then(|lease| lease.control.track());
                let _tracking = tracking.ok_or_else(|| WireError::NotController {
                    workbench: id.clone(),
                })?;
                workbenches::put_snapshot(self.host, id, snapshot).await
            }
            _ => Err(WireError::NotController { workbench: id }),
        }
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
        let id = session_id.clone();
        self.on_host(move |host| {
            host.lsp_start(
                Some(owner),
                id,
                folder_path,
                server_definition_id,
                root_path,
                lease.events,
            )
        })
        .await?;
        // The stream can close while the server is starting; a process whose
        // lease is gone answers to nobody, so stop it instead of leaking it.
        if !self.context.lsp.is_current(&session_id, lease.token) {
            let _ = self.on_host(move |host| host.lsp_stop(session_id)).await;
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
        self.on_host(move |host| host.lsp_stop(session_id)).await
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
        let persisted = tokio::task::spawn_blocking(move || -> Result<bool, WireError> {
            let valid = auth::consume_pairing_token(&config_dir, &token).map_err(|error| {
                WireError::Io {
                    message: format!("consume pairing token: {error}"),
                }
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
    use crate::test_support::{isolated, pending};

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
        assert!(pending(&mut waiting).await);
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
        assert!(pending(&mut waiting).await);
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
        assert!(pending(&mut stop).await);
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
        let mut session = Session::new(Some(Fingerprint([0; 32])), true, SessionScope::Quic);
        session.subscriber_id = id.into();
        Mutex::new(session)
    }

    #[test]
    fn attach_recovery_is_connection_bound_and_supersession_safe() {
        if !isolated("dispatch::tests::attach_recovery_is_connection_bound_and_supersession_safe") {
            return;
        }
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                use sworm_protocol::rpc::{AttachMode, WorkbenchAttached};
                let scratch = tempfile::tempdir().unwrap();
                let host = Arc::new(
                    Host::new(scratch.path().join("server.db"), Arc::new(|_| Ok(()))).unwrap(),
                );
                let (events, _) = tokio::sync::broadcast::channel(4);
                let context = Arc::new(ServerContext::new(PathBuf::new(), None, events));
                let session = paired_session("desktop");
                let runtime = DispatchRuntime {
                    host: &host,
                    context: &context,
                    session: &session,
                    scope: SessionScope::Quic,
                };
                let id = "recovery".to_owned();
                let gate = Arc::new(Mutex::new(()));
                session.lock().await.attach_attempts.insert(
                    id.clone(),
                    Arc::new(AttachAttempt {
                        attachment_id: "seed".into(),
                        gate: Arc::clone(&gate),
                        finished: watch::channel(true).0,
                    }),
                );
                let blocked = gate.lock().await;
                let attach = runtime.workbench_attach(
                    id.clone(),
                    "first".into(),
                    AttachMode::Open {},
                    "desktop".into(),
                );
                tokio::pin!(attach);
                assert!(pending(&mut attach).await);
                let recover = runtime.workbench_recover(id.clone(), "first".into());
                tokio::pin!(recover);
                assert!(pending(&mut recover).await);
                drop(blocked);
                let (attached, recovered) = tokio::join!(&mut attach, &mut recover);
                let attached = attached.unwrap();
                assert_eq!(recovered.unwrap(), Some(attached.clone()));
                let WorkbenchAttached::Ready {
                    controller_token, ..
                } = attached
                else {
                    panic!("not ready")
                };

                let saved = r#"{"version":4,"activeTabIndex":0,"tabs":[{"folderPath":"/tmp"}]}"#;
                runtime
                    .workbench_save(id.clone(), saved.into())
                    .await
                    .unwrap();
                assert!(matches!(
                    runtime.workbench_recover(id.clone(), "first".into()).await.unwrap(),
                    Some(WorkbenchAttached::Ready { snapshot, controller_token: token, .. })
                        if snapshot == saved && token == controller_token
                ));
                let other = paired_session("other");
                assert_eq!(
                    handle(
                        &host,
                        &context,
                        &other,
                        Request::WorkbenchRecover {
                            id: id.clone(),
                            attachment_id: "first".into(),
                        }
                    )
                    .await
                    .unwrap()
                    .workbench_recover()
                    .unwrap(),
                    None
                );
                other.lock().await.authorized = false;
                assert!(matches!(
                    handle(
                        &host,
                        &context,
                        &other,
                        Request::WorkbenchRecover {
                            id: id.clone(),
                            attachment_id: "first".into(),
                        }
                    )
                    .await,
                    Err(WireError::Unauthorized { .. })
                ));

                let resumed = runtime
                    .workbench_attach(
                        id.clone(),
                        "resumed".into(),
                        AttachMode::Resume {
                            controller_token: controller_token.clone(),
                        },
                        "desktop".into(),
                    )
                    .await
                    .unwrap();
                assert!(
                    matches!(resumed, WorkbenchAttached::Ready { controller_token: token, .. }
                    if token == controller_token)
                );
                runtime
                    .workbench_detach(id.clone(), "first".into())
                    .await
                    .unwrap();
                assert!(runtime
                    .workbench_recover(id.clone(), "resumed".into())
                    .await
                    .unwrap()
                    .is_some());
                assert_eq!(
                    runtime
                        .workbench_recover(id.clone(), "first".into())
                        .await
                        .unwrap(),
                    None
                );

                // Hold admitted work so takeover has retired the old controller
                // but cannot yet publish. A later attempt supersedes its recovery.
                let old = {
                    let leases = session.lock().await.leases.clone();
                    let held = leases.lock().await;
                    Arc::clone(&held.get(&id).unwrap().control)
                };
                let tracking = old.track().unwrap();
                let first = runtime.workbench_attach(
                    id.clone(),
                    "superseded".into(),
                    AttachMode::Takeover {},
                    "desktop".into(),
                );
                tokio::pin!(first);
                tokio::select! {
                    _ = old.retired() => {}
                    result = &mut first => panic!("takeover escaped drain: {result:?}"),
                }
                let lost = runtime.workbench_recover(id.clone(), "superseded".into());
                tokio::pin!(lost);
                assert!(pending(&mut lost).await);
                let second = runtime.workbench_attach(
                    id.clone(),
                    "winner".into(),
                    AttachMode::Takeover {},
                    "desktop".into(),
                );
                tokio::pin!(second);
                assert!(pending(&mut second).await);
                assert_eq!(lost.await.unwrap(), None);
                drop(tracking);
                let (first, second) = tokio::join!(&mut first, &mut second);
                assert_eq!(
                    first.unwrap_err(),
                    WireError::NotController {
                        workbench: id.clone()
                    }
                );
                assert!(
                    matches!(second.unwrap(), WorkbenchAttached::Ready { attachment_id, .. }
                    if attachment_id == "winner")
                );
                assert_eq!(session.lock().await.attach_attempts.len(), 1);
                runtime
                    .workbench_detach(id.clone(), "superseded".into())
                    .await
                    .unwrap();
                assert!(runtime
                    .workbench_recover(id.clone(), "winner".into())
                    .await
                    .unwrap()
                    .is_some());
                runtime
                    .workbench_detach(id.clone(), "winner".into())
                    .await
                    .unwrap();
                assert_eq!(
                    runtime
                        .workbench_recover(id.clone(), "winner".into())
                        .await
                        .unwrap(),
                    None
                );
                assert_eq!(
                    runtime
                        .workbench_recover("unknown".into(), "unknown".into())
                        .await
                        .unwrap(),
                    None
                );
                assert!(!workbenches::is_registered(&host, "unknown").await.unwrap());
                // Recovery may reach dispatch before the original attach has
                // acquired its RPC permit. Its None fences that late arrival,
                // even after a newer operation takes control.
                let late_id = "late-arrival".to_owned();
                assert_eq!(
                    runtime
                        .workbench_recover(late_id.clone(), "late".into())
                        .await
                        .unwrap(),
                    None
                );
                runtime
                    .workbench_attach(
                        late_id.clone(),
                        "newer".into(),
                        AttachMode::Open {},
                        "desktop".into(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    runtime
                        .workbench_attach(
                            late_id.clone(),
                            "late".into(),
                            AttachMode::Takeover {},
                            "desktop".into(),
                        )
                        .await
                        .unwrap_err(),
                    WireError::NotController {
                        workbench: late_id.clone()
                    }
                );
                assert!(runtime
                    .workbench_recover(late_id.clone(), "newer".into())
                    .await
                    .unwrap()
                    .is_some());
                assert_eq!(
                    runtime
                        .workbench_recover(late_id.clone(), "another-unknown".into())
                        .await
                        .unwrap(),
                    None
                );
                assert!(runtime
                    .workbench_recover(late_id.clone(), "newer".into())
                    .await
                    .unwrap()
                    .is_some());

                // Saturation preserves cancellation identities and admitted
                // controls; no unsafe eviction permits a cancelled takeover.
                while session.lock().await.cancelled_attachment_count < MAX_CANCELLED_ATTACHMENTS {
                    let count = session.lock().await.cancelled_attachment_count;
                    assert_eq!(
                        runtime
                            .workbench_recover(late_id.clone(), format!("cancelled-{count}"))
                            .await
                            .unwrap(),
                        None
                    );
                }
                assert_eq!(
                    runtime
                        .workbench_recover(late_id.clone(), "overflow".into())
                        .await
                        .unwrap_err(),
                    recovery_capacity()
                );
                assert_eq!(
                    runtime
                        .workbench_attach(
                            late_id.clone(),
                            "overflow".into(),
                            AttachMode::Takeover {},
                            "desktop".into(),
                        )
                        .await
                        .unwrap_err(),
                    recovery_capacity()
                );
                assert_eq!(
                    runtime
                        .workbench_attach(
                            late_id.clone(),
                            "late".into(),
                            AttachMode::Takeover {},
                            "desktop".into(),
                        )
                        .await
                        .unwrap_err(),
                    WireError::NotController {
                        workbench: late_id.clone()
                    }
                );
                assert_eq!(
                    runtime
                        .workbench_recover(late_id.clone(), "late".into())
                        .await
                        .unwrap(),
                    None
                );
                assert!(runtime
                    .workbench_recover(late_id.clone(), "newer".into())
                    .await
                    .unwrap()
                    .is_some());
                runtime
                    .workbench_detach(late_id, "newer".into())
                    .await
                    .unwrap();
                let watchers = session.lock().await.lease_watchers.clone();
                watchers.close();
                watchers.wait().await;
                context.workbenches.shutdown().await;
            });
    }

    // Run in a child so HOME/XDG never mutate the lib-test process shared by
    // other concurrently running unit tests.
    #[test]
    fn folder_ownership_and_concurrent_rearm() {
        if !isolated("dispatch::tests::folder_ownership_and_concurrent_rearm") {
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
                    scope: SessionScope::Quic,
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
                assert!(pending(&mut releasing).await);
                let other = scratch.path().join("other");
                std::fs::create_dir(&other).unwrap();
                let other_path = other.to_string_lossy().into_owned();
                tokio::time::timeout(std::time::Duration::from_secs(2), claim(&b, &other_path))
                    .await
                    .unwrap();
                host.tasks_list(path.clone()).unwrap();
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
