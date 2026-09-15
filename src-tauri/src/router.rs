use crate::remote_runs::{RemoteRunKind, RemoteRunService};
use crate::services::app_state_kv::AppStateKvService;
use parking_lot::Mutex;
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering as AtomicOrdering},
        Arc, LazyLock, Weak,
    },
    time::Duration,
};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    services::{
        pty::PtyRunState, settings::SettingsService,
        settings_resolution::resolve_effective_settings_for_folder_path,
    },
    Host,
};
use sworm_protocol::{
    pty::PtyEvent,
    rpc::{HostEventFrame, HostEventWire, Open, Reply, Request, RunStatus},
    settings::{merge_desktop_sections, tag_host_diagnostics, EffectiveSettingsInput},
};
use sworm_remote::{wire::read_frame, Fingerprint, Identity, RemoteClient, RemoteError};
use tokio::{
    sync::{Mutex as AsyncMutex, OnceCell},
    task::{JoinHandle, JoinSet},
    time::sleep,
};

const ADDRESS_STAGGER: Duration = Duration::from_millis(250);
pub(crate) const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
pub(crate) const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
const PENDING_STOPS_KEY: &str = "remote:pendingStops";

pub enum Target<'a> {
    Local,
    Remote { server: &'a str, path: &'a str },
}

impl<'a> Target<'a> {
    /// Parse a remote workspace URI; ordinary filesystem paths stay local.
    pub fn parse(project_path: &'a str) -> Result<Self, ApiError> {
        let Some(remote_path) = project_path.strip_prefix("sworm://") else {
            return Ok(Self::Local);
        };
        let Some(separator) = remote_path.find('/') else {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid remote path: {project_path}"
            )));
        };
        let server = &remote_path[..separator];
        if server.is_empty() {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid remote path: {project_path}"
            )));
        }
        Ok(Self::Remote {
            server,
            path: &remote_path[separator..],
        })
    }

    pub fn remote_uri(server: &str, path: &str) -> String {
        format!("sworm://{server}/{}", path.trim_start_matches('/'))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct RemoteConfig {
    address: String,
    fingerprint: String,
}

struct CachedRemote {
    config: RemoteConfig,
    client: Arc<RemoteClient>,
}

#[derive(Clone, Default)]
struct FolderClaim {
    dirs: Option<Vec<String>>,
    git: bool,
}

struct RemoteSlot {
    cached: AsyncMutex<Option<CachedRemote>>,
    claims: Mutex<HashMap<String, FolderClaim>>,
    events: Mutex<Option<JoinHandle<()>>>,
}

impl RemoteSlot {
    fn new() -> Self {
        Self {
            cached: AsyncMutex::new(None),
            claims: Mutex::new(HashMap::new()),
            events: Mutex::new(None),
        }
    }

    fn stop_events(&self) {
        if let Some(task) = self.events.lock().take() {
            task.abort();
        }
    }
}

impl Drop for RemoteSlot {
    fn drop(&mut self) {
        if let Some(task) = self.events.get_mut().take() {
            task.abort();
        }
    }
}

struct SettingsCache {
    generation: u64,
    remotes: HashMap<String, RemoteConfig>,
}

/// A stop the daemon never acknowledged. It outlives the tab, the window, and
/// the app itself: a closed tab must never strand a process on a server.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PendingStop {
    server: String,
    run_id: String,
    kind: RemoteRunKind,
}

pub(crate) struct RouterInner {
    pub(crate) host: Arc<Host>,
    /// Bound on first remote use: `quinn` needs a live Tokio runtime, and app
    /// setup runs outside one.
    endpoint: LazyLock<quinn::Endpoint>,
    remotes: Mutex<HashMap<String, Arc<RemoteSlot>>>,
    settings: AsyncMutex<SettingsCache>,
    identity: OnceCell<Arc<Identity>>,
    events: EventSink<HostEvent>,
    pub(crate) remote_runs: RemoteRunService,
    pub(crate) remote_lsp: crate::remote_lsp::RemoteLspService,
    pending_stops: Mutex<HashMap<String, PendingStop>>,
    pending_stop_running: AtomicBool,
}

#[derive(Clone)]
pub struct WorkspaceRouter {
    inner: Arc<RouterInner>,
}

impl WorkspaceRouter {
    pub fn new(host: Arc<Host>) -> Self {
        Self::with_events(host, Arc::new(|_| Ok(())))
    }

    pub fn with_events(host: Arc<Host>, events: EventSink<HostEvent>) -> Self {
        let pending_stops = load_pending_stops(&host);
        Self {
            inner: Arc::new(RouterInner {
                host,
                endpoint: LazyLock::new(client_endpoint),
                remotes: Mutex::new(HashMap::new()),
                settings: AsyncMutex::new(SettingsCache {
                    generation: u64::MAX,
                    remotes: HashMap::new(),
                }),
                identity: OnceCell::new(),
                events,
                remote_runs: RemoteRunService::new(),
                remote_lsp: crate::remote_lsp::RemoteLspService::new(),
                pending_stops: Mutex::new(pending_stops),
                pending_stop_running: AtomicBool::new(false),
            }),
        }
    }

    /// Resume stops that never reached their daemon: an outage during a tab
    /// close, or a crash before the retry landed.
    pub fn retry_pending_stops(&self) {
        self.inner.drain_pending_stops();
    }

    pub async fn session_write(&self, run_id: String, data: Vec<u8>) -> Result<(), ApiError> {
        self.inner.host.session_write(run_id, data).await
    }

    pub async fn session_resize(
        &self,
        run_id: String,
        cols: u16,
        rows: u16,
    ) -> Result<(), ApiError> {
        self.inner.host.session_resize(run_id, cols, rows).await
    }

    pub async fn tasks_write(&self, run_id: String, data: Vec<u8>) -> Result<(), ApiError> {
        self.inner.host.tasks_write(run_id, data).await
    }

    pub async fn tasks_resize(&self, run_id: String, cols: u16, rows: u16) -> Result<(), ApiError> {
        self.inner.host.tasks_resize(run_id, cols, rows).await
    }

    /// Ordered client messages: a remote session's stream, a local server's
    /// stdin. Not a table op because ordering rules out a fresh RPC stream.
    pub async fn lsp_send(&self, session_id: String, message_json: String) -> Result<(), ApiError> {
        if let Some(result) = self.inner.remote_lsp.send(&session_id, &message_json) {
            return result;
        }
        self.inner.host.lsp_send(session_id, message_json).await
    }

    /// A closing window drops its language servers on every host it reached.
    pub fn release_lsp_owner(&self, owner_id: &str) {
        self.inner.remote_lsp.release_owner(owner_id);
    }

    pub fn server_for_run(&self, run_id: &str) -> Option<String> {
        self.inner.remote_runs.server_for(run_id)
    }

    /// Force the same QUIC close path used by a real network loss.
    #[doc(hidden)]
    pub async fn close_remote_for_test(&self, server: &str) -> bool {
        let slot = self.inner.remotes.lock().get(server).cloned();
        let Some(slot) = slot else {
            return false;
        };
        let cached = slot.cached.lock().await;
        let Some(cached) = cached.as_ref() else {
            return false;
        };
        cached.client.close();
        true
    }

    /// Stops still waiting for their daemon to acknowledge them.
    #[doc(hidden)]
    pub fn pending_stops_for_test(&self) -> usize {
        self.inner.pending_stops.lock().len()
    }

    /// Drop a folder's remote claim once its last owner released it, so a
    /// reconnect stops re-subscribing watchers nothing is listening to.
    pub fn release_folder(&self, folder_path: &str) {
        let Ok(Target::Remote { server, path }) = Target::parse(folder_path) else {
            return;
        };
        let Some(slot) = self.inner.remotes.lock().get(server).cloned() else {
            return;
        };
        let mut claims = slot.claims.lock();
        claims.remove(path);
        let idle = claims.is_empty();
        drop(claims);
        if idle {
            slot.stop_events();
        }
    }

    async fn call_reply(&self, server: &str, request: Request) -> Result<Reply, ApiError> {
        self.inner.call_reply(server, request).await
    }

    /// A remote mutation runs inside the daemon's `Host`, whose `FileMoved`
    /// and `FileDeleted` events are local bookkeeping the events stream never
    /// carries. Re-emitting them here against workspace URIs keeps window
    /// claims and the editor's path tracking identical to a local folder.
    fn emit(&self, event: HostEvent) -> Result<(), ApiError> {
        (self.inner.events)(event).map_err(ApiError::Internal)
    }

    async fn stop_registered(
        &self,
        run_id: String,
        local_kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let Some(info) = self.inner.remote_runs.info(&run_id) else {
            return match local_kind {
                RemoteRunKind::Session => self.inner.host.session_stop(run_id).await,
                RemoteRunKind::Task => self.inner.host.tasks_stop(run_id).await,
            };
        };
        if let Some(stopping) = &info.stopping {
            stopping.store(true, std::sync::atomic::Ordering::Release);
        }
        let (remote_result, local_result) =
            tokio::join!(self.inner.stop_backend(&run_id, info.kind), async {
                match info.kind {
                    RemoteRunKind::Session => self.inner.host.session_stop(run_id.clone()).await,
                    RemoteRunKind::Task => self.inner.host.tasks_stop(run_id.clone()).await,
                }
            });
        self.inner.remote_runs.cancel(&run_id, info.generation);
        if let Err(error) = &remote_result {
            self.inner
                .remember_failed_stop(&info.server, &run_id, info.kind, error);
        }
        remote_result.and(local_result)
    }

    fn local_run_status(&self, run_id: &str) -> RunStatus {
        match self.inner.host.pty.run_state(run_id) {
            Some(PtyRunState::Live) => RunStatus {
                live: true,
                exited: None,
            },
            Some(PtyRunState::Completed(code)) => RunStatus {
                live: false,
                exited: Some(code),
            },
            None => RunStatus {
                live: false,
                exited: None,
            },
        }
    }
}

impl RouterInner {
    fn slot(&self, server: &str) -> Arc<RemoteSlot> {
        Arc::clone(
            self.remotes
                .lock()
                .entry(server.to_owned())
                .or_insert_with(|| Arc::new(RemoteSlot::new())),
        )
    }

    async fn call_reply(&self, server: &str, request: Request) -> Result<Reply, ApiError> {
        let client = self.client(server).await?;
        match client.call(&request).await {
            Ok(value) => return Ok(value),
            Err(error) if matches!(&error, RemoteError::Connection(_)) || client.is_closed() => {
                self.evict(server, &client).await
            }
            Err(error) => return Err(remote_error(server, error)),
        }

        let client = self.client(server).await?;
        let result = client.call(&request).await;
        if matches!(&result, Err(RemoteError::Connection(_))) || client.is_closed() {
            self.evict(server, &client).await;
        }
        result.map_err(|error| remote_error(server, error))
    }

    pub(crate) async fn stop_backend(
        &self,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let server = self
            .remote_runs
            .server_for(run_id)
            .ok_or_else(|| ApiError::NotFound(format!("Unknown remote run `{run_id}`")))?;
        self.stop_backend_on(&server, run_id, kind).await
    }

    async fn stop_backend_on(
        &self,
        server: &str,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let request = kind.stop_request(run_id.to_owned());
        let reply = self.call_reply(server, request).await?;
        match kind {
            RemoteRunKind::Session => reply.session_stop(),
            RemoteRunKind::Task => reply.tasks_stop(),
        }
        .map_err(ApiError::from)
    }

    /// Remember a stop the daemon never acknowledged, then keep retrying it.
    pub(crate) fn remember_failed_stop(
        self: &Arc<Self>,
        server: &str,
        run_id: &str,
        kind: RemoteRunKind,
        error: &ApiError,
    ) {
        if matches!(error, ApiError::NotFound(_)) {
            // Neither the run nor the server exists any more: nothing to kill.
            return;
        }
        let mut pending = self.pending_stops.lock();
        pending.insert(
            run_id.to_owned(),
            PendingStop {
                server: server.to_owned(),
                run_id: run_id.to_owned(),
                kind,
            },
        );
        self.persist_pending_stops(&pending);
        drop(pending);
        self.drain_pending_stops();
    }

    fn forget_pending_stop(&self, run_id: &str) {
        let mut pending = self.pending_stops.lock();
        if pending.remove(run_id).is_some() {
            self.persist_pending_stops(&pending);
        }
    }

    fn persist_pending_stops(&self, pending: &HashMap<String, PendingStop>) {
        let entries: Vec<&PendingStop> = pending.values().collect();
        let json = match serde_json::to_string(&entries) {
            Ok(json) => json,
            Err(error) => {
                tracing::error!(%error, "failed to encode pending remote stops");
                return;
            }
        };
        let db = self.host.db.write();
        if let Err(error) = AppStateKvService::new().put(db.conn(), PENDING_STOPS_KEY, &json) {
            tracing::error!(%error, "failed to persist pending remote stops");
        }
    }

    fn drain_pending_stops(self: &Arc<Self>) {
        if self.pending_stops.lock().is_empty() {
            return;
        }
        if self.pending_stop_running.swap(true, AtomicOrdering::AcqRel) {
            return;
        }
        let router = Arc::downgrade(self);
        // Tauri's runtime: the first drain runs from setup, outside tokio.
        tauri::async_runtime::spawn(async move {
            retry_pending_stops(router).await;
        });
    }

    pub(crate) async fn client(&self, server: &str) -> Result<Arc<RemoteClient>, ApiError> {
        self.refresh_settings().await?;
        let config = {
            let settings = self.settings.lock().await;
            settings.remotes.get(server).cloned()
        }
        .ok_or_else(|| ApiError::NotFound(format!("Unknown remote server `{server}`")))?;
        let fingerprint = Fingerprint::from_str(&config.fingerprint).map_err(|_| {
            ApiError::InvalidArgument(format!("Invalid fingerprint for remote `{server}`"))
        })?;
        let slot = self.slot(server);
        let mut cached = slot.cached.lock().await;
        if let Some(existing) = cached.as_ref() {
            if existing.config == config && !existing.client.is_closed() {
                return Ok(Arc::clone(&existing.client));
            }
        }
        if let Some(stale) = cached.take() {
            stale.client.close();
        }

        let addresses: Vec<_> = tokio::net::lookup_host(config.address.as_str())
            .await
            .map_err(|error| {
                ApiError::Remote(format!(
                    "{server}: cannot resolve {}: {error}",
                    config.address
                ))
            })?
            .collect();
        if addresses.is_empty() {
            return Err(ApiError::Remote(format!(
                "{server}: cannot resolve {}",
                config.address
            )));
        }
        let identity = self
            .identity
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(|| {
                    let dir = SettingsService::global_config_dir().map_err(ApiError::Internal)?;
                    Identity::load_or_generate(&dir, "client")
                        .map(Arc::new)
                        .map_err(|error| ApiError::Remote(format!("client identity: {error}")))
                })
                .await
                .map_err(|error| ApiError::Internal(error.to_string()))?
            })
            .await?;
        let client = connect_happy(
            &self.endpoint,
            interleave_addresses(addresses),
            Arc::clone(identity),
            fingerprint,
        )
        .await
        .map(Arc::new)
        .map_err(|error| remote_error(server, error))?;
        *cached = Some(CachedRemote {
            config,
            client: Arc::clone(&client),
        });
        Ok(client)
    }

    async fn refresh_settings(&self) -> Result<(), ApiError> {
        let generation = self.host.settings_generation();
        let mut settings = self.settings.lock().await;
        if settings.generation == generation {
            return Ok(());
        }
        let remotes = tokio::task::spawn_blocking(resolve_remote_configs)
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))??;
        settings.generation = generation;
        settings.remotes = remotes.clone();
        drop(settings);

        let slots: Vec<_> = self
            .remotes
            .lock()
            .iter()
            .map(|(server, slot)| (server.clone(), Arc::clone(slot)))
            .collect();
        for (server, slot) in slots {
            let expected = remotes.get(&server);
            if expected.is_none() {
                self.remotes.lock().remove(&server);
                slot.stop_events();
            }
            let mut cached = slot.cached.lock().await;
            if cached
                .as_ref()
                .is_some_and(|cached| Some(&cached.config) != expected)
            {
                if let Some(stale) = cached.take() {
                    stale.client.close();
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn evict(&self, server: &str, failed: &Arc<RemoteClient>) {
        let slot = self.remotes.lock().get(server).cloned();
        let Some(slot) = slot else {
            return;
        };
        let mut cached = slot.cached.lock().await;
        if cached
            .as_ref()
            .is_some_and(|cached| Arc::ptr_eq(&cached.client, failed))
        {
            if let Some(stale) = cached.take() {
                stale.client.close();
            }
        }
    }

    fn remember_claim(
        self: &Arc<Self>,
        server: &str,
        folder: &str,
        dirs: Option<Vec<String>>,
        git: bool,
    ) {
        let slot = self.slot(server);
        let mut claims = slot.claims.lock();
        let claim = claims.entry(folder.to_owned()).or_default();
        if let Some(dirs) = dirs {
            claim.dirs = Some(dirs);
        }
        claim.git |= git;
        drop(claims);
        self.ensure_events(server, &slot);
    }

    fn ensure_events(self: &Arc<Self>, server: &str, slot: &Arc<RemoteSlot>) {
        let mut task = slot.events.lock();
        if task.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let router = Arc::downgrade(self);
        let slot = Arc::downgrade(slot);
        let server = server.to_owned();
        *task = Some(tokio::spawn(async move {
            run_events(router, slot, server).await;
        }));
    }
}

macro_rules! define_router_operation {
    (
        #[route(project_path)]
        FilesWatchDirs => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $dirs:ident: $dirs_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            subscriber_id: String,
            $project_path: $project_path_type,
            $dirs: $dirs_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, Some($dirs.clone()), false);
                return self
                    .call_reply(
                        server,
                        Request::FilesWatchDirs {
                            project_path: path.to_owned(),
                            dirs: $dirs,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.inner
                .host
                .$method(subscriber_id, $project_path, $dirs)
                .await
        }
    };
    (
        #[route(project_path)]
        GitWatch => $method:ident(
            $project_path:ident: $project_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            owned: impl FnOnce(&Path) -> bool + Send + 'static,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, true);
                return self
                    .call_reply(
                        server,
                        Request::GitWatch {
                            project_path: path.to_owned(),
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.inner.host.$method($project_path, owned).await
        }
    };
    (
        #[route(path)]
        FolderResolve => $method:ident($path:ident: $path_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $path: $path_type) -> Result<$return_type, ApiError> {
            if let Target::Remote {
                server,
                path: remote_path,
            } = Target::parse(&$path)?
            {
                self.inner.remember_claim(server, remote_path, None, false);
                let mut folder = self
                    .call_reply(
                        server,
                        Request::FolderResolve {
                            path: remote_path.to_owned(),
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                folder.path = Target::remote_uri(server, &folder.path);
                return Ok(folder);
            }
            self.inner.host.$method($path).await
        }
    };
    (
        #[route(path)]
        FolderListEntries => $method:ident(
            $path:ident: $path_type:ty,
            $show_hidden:ident: $show_hidden_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $path: $path_type,
            $show_hidden: $show_hidden_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote {
                server,
                path: remote_path,
            } = Target::parse(&$path)?
            {
                // Browsing claims nothing; entries come back as remote URIs so
                // the switcher keeps navigating the same workspace.
                let mut entries = self
                    .call_reply(
                        server,
                        Request::FolderListEntries {
                            path: remote_path.to_owned(),
                            show_hidden: $show_hidden,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                for entry in &mut entries {
                    entry.path = Target::remote_uri(server, &entry.path);
                }
                return Ok(entries);
            }
            self.inner.host.$method($path, $show_hidden).await
        }
    };
    (
        #[route(folder_path)]
        SessionStart => $method:ident(
            $run_id:ident: $run_id_type:ty,
            $folder_path:ident: $folder_path_type:ty,
            $provider_id:ident: $provider_id_type:ty,
            $resume_token:ident: $resume_token_type:ty,
            $cols:ident: $cols_type:ty,
            $rows:ident: $rows_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        #[allow(clippy::too_many_arguments)]
        pub async fn $method(
            &self,
            $run_id: $run_id_type,
            $folder_path: $folder_path_type,
            $provider_id: $provider_id_type,
            $resume_token: $resume_token_type,
            $cols: $cols_type,
            $rows: $rows_type,
            output: EventSink<Vec<u8>>,
            events: EventSink<PtyEvent>,
            owner_id: Option<String>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                self.inner.remote_runs.ensure_startable(&$run_id)?;
                self.inner.remember_claim(server, path, None, false);
                let result = self
                    .call_reply(
                        server,
                        Request::SessionStart {
                            run_id: $run_id.clone(),
                            folder_path: path.to_owned(),
                            provider_id: $provider_id,
                            resume_token: $resume_token,
                            cols: $cols,
                            rows: $rows,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                if let Err(error) = self.inner.remote_runs.adopt(
                    Arc::downgrade(&self.inner),
                    &self.inner.host,
                    $run_id.clone(),
                    server.to_owned(),
                    RemoteRunKind::Session,
                    output,
                    events,
                    owner_id,
                ) {
                    // The daemon already spawned the process; nothing local
                    // owns it now, so it must not outlive the failed start.
                    let _ = self
                        .inner
                        .stop_backend(&$run_id, RemoteRunKind::Session)
                        .await;
                    return Err(error);
                }
                return Ok(result);
            }
            self.inner
                .host
                .$method(
                    $run_id,
                    $folder_path,
                    $provider_id,
                    $resume_token,
                    $cols,
                    $rows,
                    output,
                    events,
                    owner_id,
                    false,
                )
                .await
        }
    };
    (
        #[route(folder_path)]
        TasksStart => $method:ident(
            $run_id:ident: $run_id_type:ty,
            $folder_path:ident: $folder_path_type:ty,
            $task_id:ident: $task_id_type:ty,
            $active_file_path:ident: $active_file_path_type:ty,
            $cols:ident: $cols_type:ty,
            $rows:ident: $rows_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        #[allow(clippy::too_many_arguments)]
        pub async fn $method(
            &self,
            $run_id: $run_id_type,
            $folder_path: $folder_path_type,
            $task_id: $task_id_type,
            $active_file_path: $active_file_path_type,
            $cols: $cols_type,
            $rows: $rows_type,
            output: EventSink<Vec<u8>>,
            events: EventSink<PtyEvent>,
            owner_id: Option<String>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                self.inner.remote_runs.ensure_startable(&$run_id)?;
                self.inner.remember_claim(server, path, None, false);
                let result = self
                    .call_reply(
                        server,
                        Request::TasksStart {
                            run_id: $run_id.clone(),
                            folder_path: path.to_owned(),
                            task_id: $task_id,
                            active_file_path: $active_file_path,
                            cols: $cols,
                            rows: $rows,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                if let Err(error) = self.inner.remote_runs.adopt(
                    Arc::downgrade(&self.inner),
                    &self.inner.host,
                    $run_id.clone(),
                    server.to_owned(),
                    RemoteRunKind::Task,
                    output,
                    events,
                    owner_id,
                ) {
                    // The daemon already spawned the process; nothing local
                    // owns it now, so it must not outlive the failed start.
                    let _ = self.inner.stop_backend(&$run_id, RemoteRunKind::Task).await;
                    return Err(error);
                }
                return Ok(result);
            }
            self.inner
                .host
                .$method(
                    $run_id,
                    $folder_path,
                    $task_id,
                    $active_file_path,
                    $cols,
                    $rows,
                    output,
                    events,
                    owner_id,
                    false,
                )
                .await
        }
    };
    (
        #[route(run)]
        SessionStop => $method:ident($run_id:ident: $run_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $run_id: $run_id_type) -> Result<$return_type, ApiError> {
            self.stop_registered($run_id, RemoteRunKind::Session).await
        }
    };
    (
        #[route(run)]
        TasksStop => $method:ident($run_id:ident: $run_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $run_id: $run_id_type) -> Result<$return_type, ApiError> {
            self.stop_registered($run_id, RemoteRunKind::Task).await
        }
    };
    (
        #[route(run)]
        RunStatus => $method:ident($run_id:ident: $run_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $run_id: $run_id_type) -> Result<$return_type, ApiError> {
            let Some(server) = self.inner.remote_runs.server_for(&$run_id) else {
                return Ok(self.local_run_status(&$run_id));
            };
            self.call_reply(&server, Request::RunStatus { run_id: $run_id })
                .await?
                .$method()
                .map_err(ApiError::from)
        }
    };
    (
        #[route(none)]
        Pair => $method:ident(
            $token:ident: $token_type:ty,
            $name:ident: $name_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $token: $token_type,
            $name: $name_type,
        ) -> Result<$return_type, ApiError> {
            let _ = ($token, $name);
            Err(ApiError::InvalidArgument(
                "pair is connection-level and requires a remote server".to_owned(),
            ))
        }
    };
    (
        #[route(project_path)]
        FileRename => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $old_path:ident: $old_path_type:ty,
            $new_path:ident: $new_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $old_path: $old_path_type,
            $new_path: $new_path_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                self.call_reply(
                    server,
                    Request::FileRename {
                        project_path: path.to_owned(),
                        old_path: $old_path.clone(),
                        new_path: $new_path.clone(),
                    },
                )
                .await?
                .$method()
                .map_err(ApiError::from)?;
                return self.emit(HostEvent::FileMoved {
                    folder_path: PathBuf::from(Target::remote_uri(server, path)),
                    old_path: remote_child_uri(server, path, &$old_path),
                    new_path: remote_child_uri(server, path, &$new_path),
                    replace_destination: false,
                });
            }
            self.inner
                .host
                .$method($project_path, $old_path, $new_path)
                .await
        }
    };
    (
        #[route(project_path)]
        FileDelete => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $file_path:ident: $file_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $file_path: $file_path_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                self.call_reply(
                    server,
                    Request::FileDelete {
                        project_path: path.to_owned(),
                        file_path: $file_path.clone(),
                    },
                )
                .await?
                .$method()
                .map_err(ApiError::from)?;
                return self.emit(HostEvent::FileDeleted(remote_child_uri(
                    server,
                    path,
                    &$file_path,
                )));
            }
            self.inner.host.$method($project_path, $file_path).await
        }
    };
    (
        #[route(project_path)]
        FilePaste => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $target_dir:ident: $target_dir_type:ty,
            $op:ident: $op_type:ty,
            $sources:ident: $sources_type:ty,
            $collision_policy:ident: $collision_policy_type:ty,
            $rename_map:ident: $rename_map_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        #[allow(clippy::too_many_arguments)]
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $target_dir: $target_dir_type,
            $op: $op_type,
            $sources: $sources_type,
            $collision_policy: $collision_policy_type,
            $rename_map: $rename_map_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                let sources = remote_paste_sources(server, &$sources)?;
                let rename_map = match $rename_map {
                    Some(map) => Some(
                        map.into_iter()
                            .map(|(source, name)| {
                                remote_paste_source(server, &source).map(|source| (source, name))
                            })
                            .collect::<Result<HashMap<String, String>, ApiError>>()?,
                    ),
                    None => None,
                };
                let cut = $op == "cut";
                let mut mappings = self
                    .call_reply(
                        server,
                        Request::FilePaste {
                            project_path: path.to_owned(),
                            target_dir: $target_dir,
                            op: $op,
                            sources,
                            collision_policy: $collision_policy,
                            rename_map,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                for mapping in &mut mappings {
                    if cut {
                        self.emit(HostEvent::FileMoved {
                            folder_path: PathBuf::from(Target::remote_uri(server, path)),
                            old_path: PathBuf::from(Target::remote_uri(server, &mapping.source)),
                            new_path: remote_child_uri(server, path, &mapping.destination),
                            replace_destination: true,
                        })?;
                    }
                    // Hand back the URI the caller pasted, not the daemon path.
                    mapping.source = Target::remote_uri(server, &mapping.source);
                }
                return Ok(mappings);
            }
            for source in &$sources {
                if let Target::Remote { server, .. } = Target::parse(source)? {
                    return Err(ApiError::Remote(format!(
                        "pasting remote files from `{server}` into a local workspace is not supported"
                    )));
                }
            }
            self.inner
                .host
                .$method(
                    $project_path,
                    $target_dir,
                    $op,
                    $sources,
                    $collision_policy,
                    $rename_map,
                )
                .await
        }
    };
    (
        #[route(project_path)]
        FilePasteCollisions => $method:ident(
            $project_path:ident: $project_path_type:ty,
            $target_dir:ident: $target_dir_type:ty,
            $sources:ident: $sources_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $project_path: $project_path_type,
            $target_dir: $target_dir_type,
            $sources: $sources_type,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$project_path)? {
                self.inner.remember_claim(server, path, None, false);
                let sources = remote_paste_sources(server, &$sources)?;
                let mut collisions = self
                    .call_reply(
                        server,
                        Request::FilePasteCollisions {
                            project_path: path.to_owned(),
                            target_dir: $target_dir,
                            sources,
                        },
                    )
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                for collision in &mut collisions {
                    collision.source = Target::remote_uri(server, &collision.source);
                }
                return Ok(collisions);
            }
            for source in &$sources {
                if let Target::Remote { server, .. } = Target::parse(source)? {
                    return Err(ApiError::Remote(format!(
                        "pasting remote files from `{server}` into a local workspace is not supported"
                    )));
                }
            }
            self.inner
                .host
                .$method($project_path, $target_dir, $sources)
                .await
        }
    };
    (
        #[route(input_folder_path)]
        SettingsGetEffective => $method:ident(
            $input:ident: $input_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        /// Host sections resolve where the folder lives; desktop sections
        /// describe this window, so the local layer wins for those.
        pub async fn $method(&self, $input: $input_type) -> Result<$return_type, ApiError> {
            if let Some(folder_path) = $input.folder_path.as_deref() {
                if let Target::Remote { server, path } = Target::parse(folder_path)? {
                    let mut remote = self
                        .call_reply(
                            server,
                            Request::SettingsGetEffective {
                                input: EffectiveSettingsInput {
                                    folder_path: Some(path.to_owned()),
                                },
                            },
                        )
                        .await?
                        .$method()
                        .map_err(ApiError::from)?;
                    let local = self
                        .inner
                        .host
                        .$method(EffectiveSettingsInput { folder_path: None })
                        .await?;
                    merge_desktop_sections(&mut remote, local, server);
                    return Ok(remote);
                }
            }
            self.inner.host.$method($input).await
        }
    };
    (
        #[route(input_path)]
        SettingsOpenFolderFile => $method:ident(
            $input:ident: $input_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, mut $input: $input_type) -> Result<$return_type, ApiError> {
            // Owned up front: the folder field is rewritten to the daemon path.
            let routed = match Target::parse(&$input.folder_path)? {
                Target::Local => None,
                Target::Remote { server, path } => Some((server.to_owned(), path.to_owned())),
            };
            if let Some((server, path)) = routed {
                self.inner.remember_claim(&server, &path, None, false);
                $input.folder_path = path;
                let mut result = self
                    .call_reply(&server, Request::SettingsOpenFolderFile { input: $input })
                    .await?
                    .$method()
                    .map_err(ApiError::from)?;
                // The file the caller opens next lives on the daemon.
                result.path = Target::remote_uri(&server, &result.path);
                return Ok(result);
            }
            self.inner.host.$method($input).await
        }
    };
    (
        #[route(server)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        /// Host-owned settings belong to whichever server runs the workspace
        /// being edited, so the caller names it instead of a path.
        pub async fn $method(
            &self,
            server: Option<String>,
            $($argument: $argument_type),*
        ) -> Result<$return_type, ApiError> {
            if let Some(server) = server.as_deref() {
                return self
                    .call_reply(server, Request::$variant { $($argument),* })
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.inner.host.$method($($argument),*).await
        }
    };
    (
        #[route(opt_folder_path)]
        $variant:ident => $method:ident(
            $folder_path:ident: $folder_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $folder_path: $folder_path_type,
        ) -> Result<$return_type, ApiError> {
            if let Some(folder_path) = $folder_path.as_deref() {
                if let Target::Remote { server, path } = Target::parse(folder_path)? {
                    self.inner.remember_claim(server, path, None, false);
                    return self
                        .call_reply(
                            server,
                            Request::$variant {
                                $folder_path: Some(path.to_owned()),
                            },
                        )
                        .await?
                        .$method()
                        .map_err(ApiError::from);
                }
            }
            self.inner.host.$method($folder_path).await
        }
    };
    (
        #[route(folder_path)]
        LspStart => $method:ident(
            $session_id:ident: $session_id_type:ty,
            $folder_path:ident: $folder_path_type:ty,
            $server_definition_id:ident: $server_definition_id_type:ty,
            $root_path:ident: $root_path_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            owner_id: Option<String>,
            $session_id: $session_id_type,
            $folder_path: $folder_path_type,
            $server_definition_id: $server_definition_id_type,
            $root_path: $root_path_type,
            events: EventSink<sworm_protocol::lsp::LspEvent>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                self.inner.remember_claim(server, path, None, false);
                // The language server only ever sees daemon-absolute paths.
                let root_path = crate::remote_lsp::daemon_root_path(server, &$root_path);
                // Register before starting: the daemon rejects a start whose
                // event stream is missing.
                self.inner
                    .remote_lsp
                    .attach(&self.inner, &$session_id, server, owner_id, events)
                    .await?;
                let result = match self
                    .call_reply(
                        server,
                        Request::LspStart {
                            session_id: $session_id.clone(),
                            folder_path: path.to_owned(),
                            server_definition_id: $server_definition_id,
                            root_path,
                        },
                    )
                    .await
                {
                    Ok(reply) => reply.$method().map_err(ApiError::from),
                    Err(error) => Err(error),
                };
                if result.is_err() {
                    self.inner.remote_lsp.cancel(&$session_id);
                }
                return result;
            }
            self.inner
                .host
                .$method(
                    owner_id,
                    $session_id,
                    $folder_path,
                    $server_definition_id,
                    $root_path,
                    events,
                )
                .await
        }
    };
    (
        #[route(lsp)]
        LspStop => $method:ident($session_id:ident: $session_id_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $session_id: $session_id_type) -> Result<$return_type, ApiError> {
            let Some(server) = self.inner.remote_lsp.server_for(&$session_id) else {
                return self.inner.host.$method($session_id).await;
            };
            // The stream is this session's lease: once it is gone the daemon
            // already killed the server, and a stop for the id could only reach
            // the session that replaced this one.
            if self.inner.remote_lsp.cancel_if_ended(&$session_id) {
                return Ok(Default::default());
            }
            let result = match self
                .call_reply(
                    &server,
                    Request::LspStop {
                        session_id: $session_id.clone(),
                    },
                )
                .await
            {
                Ok(reply) => reply.$method().map_err(ApiError::from),
                Err(error) => Err(error),
            };
            // Closing the stream is the backstop kill, so drop it either way.
            self.inner.remote_lsp.cancel(&$session_id);
            result
        }
    };
    (
        #[route($route:ident)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $($argument: $argument_type),*
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$route)? {
                self.inner.remember_claim(server, path, None, false);
                let $route = path.to_owned();
                return self
                    .call_reply(server, Request::$variant { $($argument),* })
                    .await?
                    .$method()
                    .map_err(ApiError::from);
            }
            self.inner.host.$method($($argument),*).await
        }
    };
}

macro_rules! define_router_operations {
    (
        $(
            #[route($route:ident)]
            $variant:ident => $method:ident(
                $($argument:ident: $argument_type:ty),* $(,)?
            ) -> $return_type:ty;
        )*
    ) => {
        impl WorkspaceRouter {
            $(
                define_router_operation! {
                    #[route($route)]
                    $variant => $method(
                        $($argument: $argument_type),*
                    ) -> $return_type;
                }
            )*
        }
    };
}

sworm_protocol::sworm_rpc_ops!(define_router_operations);

/// Load stops left unacknowledged by an earlier run of the app.
fn load_pending_stops(host: &Host) -> HashMap<String, PendingStop> {
    let stored = {
        let db = host.db.read();
        AppStateKvService::new().get(db.conn(), PENDING_STOPS_KEY)
    };
    let entries: Vec<PendingStop> = match stored {
        Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|error| {
            tracing::error!(%error, "failed to decode pending remote stops");
            Vec::new()
        }),
        Ok(None) => Vec::new(),
        Err(error) => {
            tracing::error!(%error, "failed to read pending remote stops");
            Vec::new()
        }
    };
    entries
        .into_iter()
        .map(|entry| (entry.run_id.clone(), entry))
        .collect()
}

/// Retry unacknowledged stops until every one lands. Backoff is shared with
/// reconnects because an unreachable daemon is the usual reason one is here.
async fn retry_pending_stops(router: Weak<RouterInner>) {
    let mut delay = INITIAL_RECONNECT_DELAY;
    loop {
        let Some(inner) = router.upgrade() else {
            return;
        };
        let entries: Vec<PendingStop> = inner.pending_stops.lock().values().cloned().collect();
        if entries.is_empty() {
            inner
                .pending_stop_running
                .store(false, AtomicOrdering::Release);
            // An insert may have raced the exit above; it owns the flag now.
            inner.drain_pending_stops();
            return;
        }
        for entry in entries {
            match inner
                .stop_backend_on(&entry.server, &entry.run_id, entry.kind)
                .await
            {
                // A daemon that no longer knows the run has nothing left to kill.
                Ok(()) | Err(ApiError::NotFound(_)) => inner.forget_pending_stop(&entry.run_id),
                Err(error) => tracing::warn!(
                    server = entry.server,
                    run_id = entry.run_id,
                    %error,
                    "retrying remote stop"
                ),
            }
        }
        drop(inner);
        sleep(delay).await;
        delay = (delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

async fn run_events(router: Weak<RouterInner>, slot: Weak<RemoteSlot>, server: String) {
    let mut reconnect_delay = INITIAL_RECONNECT_DELAY;
    loop {
        let (Some(router_now), Some(slot_now)) = (router.upgrade(), slot.upgrade()) else {
            return;
        };
        let client = match router_now.client(&server).await {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%server, %error, "remote events reconnect failed");
                drop(router_now);
                drop(slot_now);
                sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
                continue;
            }
        };
        let Ok((_send, mut recv)) = client.open_stream(Open::Events).await else {
            router_now.evict(&server, &client).await;
            drop(router_now);
            drop(slot_now);
            sleep(reconnect_delay).await;
            reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
            continue;
        };
        restore_claims(&server, &slot_now, &client).await;
        reconnect_delay = INITIAL_RECONNECT_DELAY;
        drop(router_now);
        drop(slot_now);

        loop {
            tokio::select! {
                frame = read_frame::<HostEventFrame>(&mut recv) => match frame {
                    Ok(HostEventFrame(event)) => {
                        let Some(router_now) = router.upgrade() else { return };
                        if let Err(error) = (router_now.events)(remote_host_event(&server, event)) {
                            tracing::warn!(%server, %error, "remote host event delivery failed");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%server, %error, "remote events stream lost");
                        break;
                    }
                },
                _ = client.closed() => break,
            }
        }
        if let Some(router_now) = router.upgrade() {
            router_now.evict(&server, &client).await;
        } else {
            return;
        }
        sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
    }
}

async fn restore_claims(server: &str, slot: &RemoteSlot, client: &RemoteClient) {
    let claims: Vec<_> = slot
        .claims
        .lock()
        .iter()
        .map(|(folder, claim)| (folder.clone(), claim.clone()))
        .collect();
    for (folder, claim) in claims {
        let claim_result = client
            .call(&Request::FolderResolve {
                path: folder.clone(),
            })
            .await
            .and_then(|reply| reply.folder_resolve().map_err(RemoteError::Wire));
        if let Err(error) = claim_result {
            tracing::warn!(%server, %folder, %error, "remote folder claim restoration failed");
            continue;
        }
        if let Some(dirs) = claim.dirs {
            let result = client
                .call(&Request::FilesWatchDirs {
                    project_path: folder.clone(),
                    dirs,
                })
                .await
                .and_then(|reply| reply.files_watch_dirs().map_err(RemoteError::Wire));
            if let Err(error) = result {
                tracing::warn!(%server, %folder, %error, "remote file watch restoration failed");
            }
        }
        if claim.git {
            let result = client
                .call(&Request::GitWatch {
                    project_path: folder.clone(),
                })
                .await
                .and_then(|reply| reply.git_watch().map_err(RemoteError::Wire));
            if let Err(error) = result {
                tracing::warn!(%server, %folder, %error, "remote git watch restoration failed");
            }
        }
    }
}

fn remote_host_event(server: &str, event: HostEventWire) -> HostEvent {
    match event {
        HostEventWire::FilesChanged(mut event) => {
            event.folder_path = Target::remote_uri(server, &event.folder_path);
            HostEvent::FilesChanged(event)
        }
        HostEventWire::GitChanged(mut event) => {
            event.folder_path = Target::remote_uri(server, &event.folder_path);
            HostEvent::GitChanged(event)
        }
        HostEventWire::SettingsChanged(mut event) => {
            event.folder_path = event
                .folder_path
                .map(|folder| Target::remote_uri(server, &folder));
            // Diagnostics from the daemon's layers must not read as local ones.
            tag_host_diagnostics(&mut event.diagnostics, server);
            HostEvent::SettingsChanged(event)
        }
        HostEventWire::TasksChanged(folder) => {
            HostEvent::TasksChanged(Target::remote_uri(server, &folder))
        }
        HostEventWire::NixChanged(folder) => {
            HostEvent::NixChanged(Target::remote_uri(server, &folder))
        }
        HostEventWire::IssuesChanged(folder) => {
            HostEvent::IssuesChanged(Target::remote_uri(server, &folder))
        }
    }
}

fn resolve_remote_configs() -> Result<HashMap<String, RemoteConfig>, ApiError> {
    let resolved = resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
    Ok(resolved
        .settings
        .remotes
        .into_iter()
        .map(|(name, remote)| {
            (
                name,
                RemoteConfig {
                    address: remote.address,
                    fingerprint: remote.fingerprint,
                },
            )
        })
        .collect())
}

fn client_endpoint() -> quinn::Endpoint {
    quinn::Endpoint::client("[::]:0".parse().expect("valid IPv6 wildcard"))
        .or_else(|_| quinn::Endpoint::client("0.0.0.0:0".parse().expect("valid IPv4 wildcard")))
        .expect("bind QUIC client endpoint")
}

fn interleave_addresses(addresses: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(first) = addresses.first() else {
        return addresses;
    };
    let first_is_v6 = matches!(first.ip(), IpAddr::V6(_));
    let (first_family, second_family): (VecDeque<_>, VecDeque<_>) = addresses
        .into_iter()
        .partition(|address| matches!(address.ip(), IpAddr::V6(_)) == first_is_v6);
    let mut families = [first_family, second_family];
    let mut ordered = Vec::with_capacity(families.iter().map(VecDeque::len).sum());
    while !families[0].is_empty() || !families[1].is_empty() {
        for family in &mut families {
            if let Some(address) = family.pop_front() {
                ordered.push(address);
            }
        }
    }
    ordered
}

async fn connect_happy(
    endpoint: &quinn::Endpoint,
    addresses: Vec<SocketAddr>,
    identity: Arc<Identity>,
    fingerprint: Fingerprint,
) -> Result<RemoteClient, RemoteError> {
    let mut attempts = JoinSet::new();
    for (index, address) in addresses.into_iter().enumerate() {
        let endpoint = endpoint.clone();
        let identity = Arc::clone(&identity);
        attempts.spawn(async move {
            if index > 0 {
                sleep(ADDRESS_STAGGER * index as u32).await;
            }
            RemoteClient::connect(&endpoint, address, &identity, fingerprint).await
        });
    }

    let mut last_error = None;
    while let Some(result) = attempts.join_next().await {
        match result {
            Ok(Ok(client)) => {
                attempts.abort_all();
                while let Some(loser) = attempts.join_next().await {
                    if let Ok(Ok(loser)) = loser {
                        loser.close();
                    }
                }
                return Ok(client);
            }
            Ok(Err(error)) => last_error = Some(error),
            Err(error) if !error.is_cancelled() => {
                last_error = Some(RemoteError::Transport(format!(
                    "connection attempt failed: {error}"
                )))
            }
            Err(_) => {}
        }
    }
    Err(last_error.unwrap_or_else(|| RemoteError::Transport("no resolved addresses".to_owned())))
}

/// Absolute URI of a workspace-relative path inside a remote folder.
fn remote_child_uri(server: &str, folder: &str, relative: &str) -> PathBuf {
    let folder = folder.trim_end_matches('/');
    let relative = relative.trim_start_matches('/');
    PathBuf::from(Target::remote_uri(server, &format!("{folder}/{relative}")))
}

/// Clipboard sources name files on the host that owns them. A local path here
/// would be a file the daemon cannot see, and another server's URI a file
/// neither host can reach, so both are refused instead of silently pasting
/// whatever happens to exist at that path on the daemon.
fn remote_paste_source(server: &str, source: &str) -> Result<String, ApiError> {
    match Target::parse(source)? {
        Target::Remote {
            server: origin,
            path,
        } if origin == server => Ok(path.to_owned()),
        Target::Remote { server: origin, .. } => Err(ApiError::Remote(format!(
            "cannot paste files from `{origin}` into a workspace on `{server}`"
        ))),
        Target::Local => Err(ApiError::Remote(format!(
            "pasting local files into a remote workspace is not supported: {source}"
        ))),
    }
}

fn remote_paste_sources(server: &str, sources: &[String]) -> Result<Vec<String>, ApiError> {
    sources
        .iter()
        .map(|source| remote_paste_source(server, source))
        .collect()
}

/// Refuse a remote target for a command that can only act on this machine.
pub fn reject_remote(command: &str, path: &str) -> Result<(), ApiError> {
    match Target::parse(path)? {
        Target::Local => Ok(()),
        Target::Remote { .. } => Err(ApiError::Remote(format!(
            "{command} is not supported on remote workspaces"
        ))),
    }
}

pub(crate) fn remote_error(server: &str, error: RemoteError) -> ApiError {
    match error {
        RemoteError::Wire(error) => ApiError::from(error),
        RemoteError::Connection(message)
        | RemoteError::Transport(message)
        | RemoteError::Identity(message) => ApiError::Remote(format!("{server}: {message}")),
    }
}
