use crate::remote_runs::{RemoteRunKind, RemoteRunService};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, LazyLock, Weak},
    time::Duration,
};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    services::{
        app_state_kv::AppStateKvService, pty::PtySubscriber, settings::SettingsService,
        settings_resolution::resolve_effective_settings_for_folder_path,
    },
    Host,
};
use sworm_protocol::{
    pty::PtyEvent,
    rpc::{
        AttachMode, HostEventFrame, HostEventWire, Open, Reply, Request, RunStatus,
        WorkbenchAttached, WorkbenchInfo,
    },
    settings::{
        merge_desktop_sections, tag_host_diagnostics, EffectiveSettingsInput, RemoteSettings,
    },
};
use sworm_remote::{wire::read_frame, Fingerprint, Identity, RemoteClient, RemoteError};
use tauri::Manager;
use tokio::{
    sync::{Mutex as AsyncMutex, OnceCell},
    task::{JoinHandle, JoinSet},
    time::sleep,
};

const ADDRESS_STAGGER: Duration = Duration::from_millis(250);
pub(crate) const INITIAL_RECONNECT_DELAY: Duration = Duration::from_secs(1);
pub(crate) const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);
const PENDING_STOPS_KEY: &str = "remote:pendingStops";
/// Each streamed assembly holds up to 256 MiB; more than this queue.
const MAX_FILE_STREAMS: usize = 2;

fn validate_server_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ApiError::InvalidArgument(
            "Server name must match [A-Za-z0-9_-]+".into(),
        ));
    }
    Ok(())
}

fn remote_entries(
    value: serde_json::Value,
) -> Result<serde_json::Map<String, serde_json::Value>, ApiError> {
    match value {
        serde_json::Value::Null => Ok(serde_json::Map::new()),
        serde_json::Value::Object(entries) => Ok(entries),
        _ => Err(ApiError::InvalidArgument(
            "remotes settings must be an object".into(),
        )),
    }
}

fn append_file_chunk(bytes: &mut Vec<u8>, chunk: &[u8], total: u64) -> Result<(), ApiError> {
    let cap = sworm_protocol::rpc::MAX_STREAM_FILE_BYTES;
    if chunk.len() > cap.saturating_sub(bytes.len())
        || bytes.len() as u64 + chunk.len() as u64 > total
    {
        return Err(ApiError::InvalidArgument(
            "File stream exceeds size limit".into(),
        ));
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

#[tauri::command]
pub async fn pair_remote(
    state: tauri::State<'_, crate::app_state::AppState>,
    link: String,
    name: String,
) -> Result<RemoteSettings, ApiError> {
    state.router.pair_remote(&link, &name, false).await
}

#[tauri::command]
pub async fn repair_remote(
    state: tauri::State<'_, crate::app_state::AppState>,
    link: String,
    name: String,
) -> Result<RemoteSettings, ApiError> {
    state.router.pair_remote(&link, &name, true).await
}

/// This desktop's identity fingerprint, which servers list in `authorized_keys`.
#[tauri::command]
pub async fn remote_client_fingerprint(
    state: tauri::State<'_, crate::app_state::AppState>,
) -> Result<String, ApiError> {
    Ok(state
        .router
        .inner
        .client_identity()
        .await?
        .fingerprint()
        .to_string())
}

#[tauri::command]
pub async fn remote_status(
    state: tauri::State<'_, crate::app_state::AppState>,
    server: String,
) -> Result<RemoteStatus, ApiError> {
    state.router.remote_status(&server).await
}

#[tauri::command]
pub async fn workbench_list(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    server: String,
) -> Result<Vec<WorkbenchInfo>, ApiError> {
    state
        .router
        .workbench_list_for_owner(window.label(), &server)
        .await
}

#[tauri::command]
pub async fn workbench_close(
    state: tauri::State<'_, crate::app_state::AppState>,
    server: String,
    id: String,
) -> Result<(), ApiError> {
    state.router.workbench_close(&server, id).await
}

#[tauri::command]
pub async fn workbench_attach(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    server: String,
    id: String,
    mode: AttachMode,
    attachment_id: String,
) -> Result<Option<WorkbenchAttached>, ApiError> {
    let attached = state
        .router
        .workbench_attach(
            window.label(),
            &server,
            id.clone(),
            mode,
            attachment_id.clone(),
        )
        .await?;
    if attached.is_none() {
        if let Some(owner) = state.router.workbench_owner(&server, &id, window.label()) {
            if let Some(other) = window.app_handle().get_webview_window(&owner) {
                other
                    .unminimize()
                    .and_then(|_| other.show())
                    .and_then(|_| other.set_focus())
                    .map_err(|error| ApiError::Internal(error.to_string()))?;
            }
        }
    }
    // The RPC may finish after the window's teardown already drained its leases.
    if matches!(attached, Some(WorkbenchAttached::Ready { .. }))
        && !state.windows.has_window(window.label())
    {
        if let Err(error) = state
            .router
            .workbench_detach(window.label(), &server, id, attachment_id)
            .await
        {
            tracing::warn!(%server, %error, "late workbench attach detach failed");
        }
    }
    Ok(attached)
}

#[tauri::command]
pub async fn workbench_detach(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    server: String,
    id: String,
    attachment_id: String,
) -> Result<(), ApiError> {
    state
        .router
        .workbench_detach(window.label(), &server, id, attachment_id)
        .await
}

#[tauri::command]
pub async fn workbench_transfer(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    target_owner: String,
    server: String,
    id: String,
    attachment_id: String,
) -> Result<WorkbenchAttached, ApiError> {
    if !state.windows.has_window(&target_owner) {
        return Err(ApiError::NotFound(format!(
            "Unknown target window `{target_owner}`"
        )));
    }
    state
        .router
        .workbench_transfer(window.label(), &target_owner, &server, id, attachment_id)
        .await
}

#[tauri::command]
pub async fn workbench_save(
    state: tauri::State<'_, crate::app_state::AppState>,
    server: String,
    id: String,
    snapshot: String,
) -> Result<(), ApiError> {
    state.router.workbench_save(&server, id, snapshot).await
}

#[tauri::command]
pub fn remote_runs_release(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    run_ids: Vec<String>,
) -> Result<(), ApiError> {
    state.router.remote_runs_release(window.label(), &run_ids)
}

#[tauri::command]
pub async fn rename_remote(
    state: tauri::State<'_, crate::app_state::AppState>,
    server: String,
    name: String,
) -> Result<(), ApiError> {
    state
        .router
        .change_remote(&server, Some(&name), &state.windows)
        .await
}

#[tauri::command]
pub async fn remove_remote(
    state: tauri::State<'_, crate::app_state::AppState>,
    server: String,
) -> Result<(), ApiError> {
    state
        .router
        .change_remote(&server, None, &state.windows)
        .await
}

#[tauri::command]
pub async fn file_read_stream(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
    project_path: String,
    file_path: String,
    version: String,
    size: u64,
) -> Result<sworm_protocol::files::FileContent, ApiError> {
    use tauri::Emitter;
    state.router.read_file_stream(window.label(), &request_id, project_path.clone(), file_path.clone(), version, size, |bytes, total| {
        let _ = window.emit("file-read-progress", serde_json::json!({
            "requestId": request_id, "folderPath": project_path, "filePath": file_path, "bytes": bytes, "total": total,
        }));
    }).await
}

#[tauri::command]
pub fn file_read_stream_cancel(
    state: tauri::State<'_, crate::app_state::AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
) {
    state.router.cancel_file_read(window.label(), &request_id);
}
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

struct CachedRemote {
    config: RemoteSettings,
    client: Arc<RemoteClient>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct RemoteStatus {
    pub connected: bool,
    pub last_error: Option<String>,
    pub state: String,
}

impl Default for RemoteStatus {
    fn default() -> Self {
        Self {
            connected: false,
            last_error: None,
            state: "disconnected".into(),
        }
    }
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
    status: Mutex<RemoteStatus>,
}

impl RemoteSlot {
    fn new() -> Self {
        Self {
            cached: AsyncMutex::new(None),
            claims: Mutex::new(HashMap::new()),
            events: Mutex::new(None),
            status: Mutex::new(RemoteStatus::default()),
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
        if let Some(cached) = self.cached.get_mut().take() {
            cached.client.close();
        }
        if let Some(task) = self.events.get_mut().take() {
            task.abort();
        }
    }
}

struct SettingsCache {
    generation: u64,
    remotes: HashMap<String, RemoteSettings>,
}

/// A stop the daemon never acknowledged. It outlives the tab, the window, and
/// the app itself: a closed tab must never strand a process on a server.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PendingStop {
    server: String,
    workbench: String,
    run_id: String,
    kind: RemoteRunKind,
    #[serde(default)]
    controller_token: String,
}

#[derive(Clone, Debug)]
struct WorkbenchLease {
    id: String,
    attachment_id: String,
    controller_token: String,
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
    leases: Mutex<HashMap<(String, String), WorkbenchLease>>,
    transitions: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    pub(crate) remote_runs: RemoteRunService,
    pub(crate) remote_lsp: crate::remote_lsp::RemoteLspService,
    pending_stops: Mutex<HashMap<String, PendingStop>>,
    pending_stop_running: Mutex<HashSet<String>>,
    remote_management: AsyncMutex<()>,
    file_reads: Mutex<HashMap<(String, String), tokio::sync::watch::Sender<bool>>>,
    file_stream_permits: tokio::sync::Semaphore,
}

#[derive(Clone)]
pub struct WorkspaceRouter {
    inner: Arc<RouterInner>,
}

impl WorkspaceRouter {
    pub fn new(host: Arc<Host>) -> Self {
        Self::with_events(host, Arc::new(|_| Ok(())))
    }
    pub async fn remote_status(&self, server: &str) -> Result<RemoteStatus, ApiError> {
        self.inner.refresh_settings().await?;
        if !self
            .inner
            .settings
            .lock()
            .await
            .remotes
            .contains_key(server)
        {
            return Err(ApiError::NotFound(format!(
                "Unknown remote server `{server}`"
            )));
        }
        let slot = self.inner.slot(server);
        let cached = slot.cached.lock().await;
        if let Some(cached) = cached.as_ref() {
            if let Some(error) = cached.client.connection().close_reason() {
                self.inner.set_status(
                    server,
                    RemoteStatus {
                        connected: false,
                        last_error: Some(error.to_string()),
                        state: "error".into(),
                    },
                );
            }
        }
        let status = slot.status.lock().clone();
        Ok(status)
    }

    pub async fn pair_remote(
        &self,
        link: &str,
        name: &str,
        replace: bool,
    ) -> Result<RemoteSettings, ApiError> {
        validate_server_name(name)?;
        let link: sworm_protocol::pairing::PairLink = link
            .parse()
            .map_err(|error| ApiError::InvalidArgument(format!("{error}")))?;
        let _guard = self.inner.remote_management.lock().await;
        let remotes = resolve_effective_settings_for_folder_path(None)
            .map_err(ApiError::Internal)?
            .settings
            .remotes;
        if remotes.contains_key(name) != replace {
            return Err(ApiError::InvalidArgument(if replace {
                format!("Remote `{name}` no longer exists; use Pair instead")
            } else {
                format!("Remote `{name}` already exists; use Re-pair instead")
            }));
        }
        let identity = self.inner.client_identity().await?;
        let addresses = tokio::net::lookup_host(link.address())
            .await
            .map_err(|error| ApiError::Remote(error.to_string()))?
            .collect();
        let fingerprint = Fingerprint::from_str(&link.fingerprint)
            .map_err(|error| ApiError::InvalidArgument(error.to_string()))?;
        let client = connect_happy(
            &self.inner.endpoint,
            interleave_addresses(addresses),
            identity,
            fingerprint,
        )
        .await
        .map_err(|error| remote_error(name, error))?;
        if let Err(error) = client.pair(&link.token, name).await {
            client.close();
            return Err(remote_error(name, error));
        }
        let entry = RemoteSettings {
            address: link.address(),
            fingerprint: link.fingerprint,
        };
        let expected = remotes
            .get(name)
            .map(serde_json::to_value)
            .transpose()
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        let persisted = self
            .inner
            .host
            .settings_update_global_section("remotes", |value| {
                let mut entries = remote_entries(value)?;
                if entries.get(name) != expected.as_ref() {
                    return Err(ApiError::InvalidArgument(
                        "Remote settings changed during pairing; retry".into(),
                    ));
                }
                entries.insert(
                    name.into(),
                    serde_json::to_value(&entry)
                        .map_err(|error| ApiError::Internal(error.to_string()))?,
                );
                Ok(serde_json::Value::Object(entries))
            });
        if let Err(error) = persisted {
            client.close();
            return Err(error);
        }
        self.inner
            .settings
            .lock()
            .await
            .remotes
            .insert(name.into(), entry.clone());
        let slot = self.inner.slot(name);
        let client = Arc::new(client);
        let mut cached = slot.cached.lock().await;
        if let Some(old) = cached.replace(CachedRemote {
            config: entry.clone(),
            client: Arc::clone(&client),
        }) {
            old.client.close();
        }
        self.inner.observe_client(name, &slot, &client);
        Ok(entry)
    }

    async fn change_remote(
        &self,
        server: &str,
        name: Option<&str>,
        windows: &crate::services::windows::WindowCoordinatorService,
    ) -> Result<(), ApiError> {
        if let Some(name) = name {
            validate_server_name(name)?;
        }
        let _guard = self.inner.remote_management.lock().await;
        if windows.remote_claimed(server) {
            return Err(ApiError::InvalidArgument(
                "Close all tabs for this remote before renaming or removing it".into(),
            ));
        }
        {
            // Held across the settings write: the retry loop resolves a stop's
            // server by name, and an unknown name reads as "run gone".
            let mut pending = self.inner.pending_stops.lock();
            if name.is_none() && pending.values().any(|stop| stop.server == server) {
                return Err(ApiError::InvalidArgument(
                    "Reconnect this remote so its pending stops land before removing it".into(),
                ));
            }
            self.inner
                .host
                .settings_update_global_section("remotes", |value| {
                    let mut entries = remote_entries(value)?;
                    if name.is_some_and(|name| entries.contains_key(name)) {
                        return Err(ApiError::InvalidArgument(
                            "Remote name already exists".into(),
                        ));
                    }
                    let entry = entries.remove(server).ok_or_else(|| {
                        ApiError::NotFound(format!("Unknown remote server `{server}`"))
                    })?;
                    if let Some(name) = name {
                        entries.insert(name.into(), entry);
                    }
                    Ok(serde_json::Value::Object(entries))
                })?;
            if let Some(name) = name {
                let mut moved = false;
                for stop in pending.values_mut().filter(|stop| stop.server == server) {
                    stop.server = name.to_owned();
                    moved = true;
                }
                if moved {
                    self.inner.persist_pending_stops(&pending);
                }
            }
        }
        self.inner.refresh_settings().await
    }

    pub fn release_file_reads(&self, owner: &str) {
        self.inner.file_reads.lock().retain(|(window, _), cancel| {
            if window != owner {
                return true;
            }
            let _ = cancel.send(true);
            false
        });
    }

    pub fn cancel_file_read(&self, owner: &str, request_id: &str) {
        let key = (owner.to_owned(), request_id.to_owned());
        let mut reads = self.inner.file_reads.lock();
        // Keep a cancelled tombstone: cancellation may arrive before invocation.
        let cancel = reads
            .entry(key)
            .or_insert_with(|| tokio::sync::watch::channel(false).0);
        cancel.send_replace(true);
    }

    pub async fn read_file_stream(
        &self,
        owner: &str,
        request_id: &str,
        project_path: String,
        file_path: String,
        version: String,
        size: u64,
        progress: impl Fn(u64, u64) + Send,
    ) -> Result<sworm_protocol::files::FileContent, ApiError> {
        if request_id.is_empty() || request_id.len() > 128 {
            return Err(ApiError::InvalidArgument(
                "Invalid file-read request id".into(),
            ));
        }
        let key = (owner.to_owned(), request_id.to_owned());
        let mut cancelled = {
            let mut reads = self.inner.file_reads.lock();
            if let Some(existing) = reads.get(&key) {
                let cancelled = *existing.borrow();
                if cancelled {
                    reads.remove(&key);
                    return Err(ApiError::InvalidArgument("File read cancelled".into()));
                }
                return Err(ApiError::InvalidArgument(
                    "File-read request id already in use".into(),
                ));
            }
            let (send, recv) = tokio::sync::watch::channel(false);
            reads.insert(key.clone(), send);
            recv
        };
        struct ReadGuard<'a>(&'a RouterInner, (String, String));
        impl Drop for ReadGuard<'_> {
            fn drop(&mut self) {
                self.0.file_reads.lock().remove(&self.1);
            }
        }
        let _guard = ReadGuard(&self.inner, key);
        let cancellation = cancelled.clone();
        let progress = move |bytes, total| {
            progress(bytes, total);
            // Buffered frames can complete in one poll, before select! polls
            // its cancellation branch again. Stop at each chunk boundary too.
            if *cancellation.borrow() {
                return Err(ApiError::InvalidArgument("File read cancelled".into()));
            }
            Ok(())
        };
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(ApiError::InvalidArgument("File read cancelled".into())),
            result = self.assemble_file_stream(project_path, file_path, version, size, progress) => {
                if *cancelled.borrow() {
                    Err(ApiError::InvalidArgument("File read cancelled".into()))
                } else {
                    result
                }
            },
        }
    }

    /// `version` and `size` are the caller's approved stat. The open on the
    /// side holding the file checks that identity once and pins it for every
    /// chunk, so nothing here stats again.
    async fn assemble_file_stream(
        &self,
        project_path: String,
        file_path: String,
        version: String,
        size: u64,
        progress: impl Fn(u64, u64) -> Result<(), ApiError> + Send,
    ) -> Result<sworm_protocol::files::FileContent, ApiError> {
        use sha2::{Digest, Sha256};
        use sworm_protocol::rpc::{FileReadDown, MAX_FILE_CHUNK_BYTES, MAX_STREAM_FILE_BYTES};
        use sworm_remote::wire::{read_tagged_frame_with_limit, Frame};
        if size > MAX_STREAM_FILE_BYTES as u64 {
            return Err(ApiError::TooLarge {
                size,
                limit: MAX_STREAM_FILE_BYTES as u64,
            });
        }
        // Held until this future ends by any path; waiting here stays
        // cancellable because the caller selects on cancellation.
        let _permit = self
            .inner
            .file_stream_permits
            .acquire()
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        let mut bytes = Vec::with_capacity(size as usize);
        progress(0, size)?;
        let final_version = if let Target::Remote { server, path } = Target::parse(&project_path)? {
            let client = self.inner.client(server).await?;
            let (mut send, mut recv) = client
                .open_stream(Open::FileRead {
                    project_path: path.into(),
                    file_path,
                    version,
                })
                .await
                .map_err(|error| remote_error(server, error))?;
            let _ = send.finish();
            let mut hash = Sha256::new();
            loop {
                match read_tagged_frame_with_limit::<FileReadDown>(&mut recv, MAX_FILE_CHUNK_BYTES)
                    .await
                    .map_err(|error| remote_error(server, error))?
                {
                    Frame::Raw(chunk) => {
                        append_file_chunk(&mut bytes, &chunk, size)?;
                        hash.update(&chunk);
                        progress(bytes.len() as u64, size)?;
                    }
                    // The daemon's version is trusted only once our own hash agrees.
                    Frame::Json(FileReadDown::Complete { version }) => {
                        if format!("{:x}", hash.finalize()) != version {
                            return Err(ApiError::Remote("File stream hash mismatch".into()));
                        }
                        break version;
                    }
                    Frame::Json(FileReadDown::Error { error }) => return Err(error.into()),
                }
            }
        } else {
            let mut reader = self
                .inner
                .host
                .file_open_read_stream(project_path, file_path, version)
                .await?;
            // Chunks land straight in the presized buffer; the reader hashes
            // exactly the bytes it appends, so its version is this content's.
            loop {
                let (next_reader, next_bytes, count) = tokio::task::spawn_blocking(move || {
                    let count = reader.read_chunk(&mut bytes);
                    (reader, bytes, count)
                })
                .await
                .map_err(|error| ApiError::Internal(error.to_string()))?;
                reader = next_reader;
                bytes = next_bytes;
                if count? == 0 {
                    break reader.version();
                }
                progress(bytes.len() as u64, size)?;
            }
        };
        if bytes.len() as u64 != size {
            return Err(ApiError::Remote("File stream size mismatch".into()));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| ApiError::InvalidArgument("File is not valid UTF-8".into()))?;
        Ok(sworm_protocol::files::FileContent {
            content,
            version: final_version,
        })
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
                leases: Mutex::new(HashMap::new()),
                transitions: Mutex::new(HashMap::new()),
                remote_runs: RemoteRunService::new(),
                remote_lsp: crate::remote_lsp::RemoteLspService::new(),
                pending_stops: Mutex::new(pending_stops),
                pending_stop_running: Mutex::new(HashSet::new()),
                remote_management: AsyncMutex::new(()),
                file_reads: Mutex::new(HashMap::new()),
                file_stream_permits: tokio::sync::Semaphore::new(MAX_FILE_STREAMS),
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
    pub fn pending_stops_for_test(&self) -> HashMap<String, String> {
        self.inner
            .pending_stops
            .lock()
            .values()
            .map(|stop| (stop.run_id.clone(), stop.server.clone()))
            .collect()
    }

    #[doc(hidden)]
    pub fn queue_stop_for_test(&self, server: &str, run_id: &str) {
        let mut pending = self.inner.pending_stops.lock();
        pending.insert(
            run_id.to_owned(),
            PendingStop {
                server: server.to_owned(),
                workbench: "test-workbench".into(),
                run_id: run_id.to_owned(),
                kind: RemoteRunKind::Session,
                controller_token: String::new(),
            },
        );
        self.inner.persist_pending_stops(&pending);
    }

    #[doc(hidden)]
    pub async fn change_remote_for_test(
        &self,
        server: &str,
        name: Option<&str>,
    ) -> Result<(), ApiError> {
        let windows = crate::services::windows::WindowCoordinatorService::new();
        self.change_remote(server, name, &windows).await
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
        let idle = claims.is_empty()
            && !self
                .inner
                .leases
                .lock()
                .keys()
                .any(|(_, lease_server)| lease_server == server);
        drop(claims);
        if idle {
            slot.stop_events();
        }
    }

    async fn call_reply(&self, server: &str, request: Request) -> Result<Reply, ApiError> {
        self.inner.call_reply(server, request).await
    }

    async fn call_run_reply(
        &self,
        server: &str,
        owner: Option<&str>,
        request: Request,
    ) -> Result<(Reply, String, tokio::sync::OwnedMutexGuard<()>), ApiError> {
        let guard = self.inner.transition(server).lock_owned().await;
        let workbench = owner
            .and_then(|owner| {
                self.inner
                    .leases
                    .lock()
                    .get(&(owner.to_owned(), server.to_owned()))
                    .map(|lease| lease.id.clone())
            })
            .ok_or_else(|| {
                ApiError::InvalidArgument(format!(
                    "Window has no attached workbench on remote `{server}`"
                ))
            })?;
        let reply = self
            .inner
            .call_reply_for(server, Some(&workbench), request)
            .await?;
        Ok((reply, workbench, guard))
    }

    pub async fn workbench_list(&self, server: &str) -> Result<Vec<WorkbenchInfo>, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.inner.reconcile(server).await
    }

    pub async fn workbench_list_for_owner(
        &self,
        owner: &str,
        server: &str,
    ) -> Result<Vec<WorkbenchInfo>, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        let mut rows = self.inner.reconcile(server).await?;
        let leases = self.inner.leases.lock();
        let lease = leases.get(&(owner.to_owned(), server.to_owned()));
        for row in &mut rows {
            row.yours &= lease.is_some_and(|lease| lease.id == row.id);
        }
        Ok(rows)
    }

    pub async fn workbench_close(&self, server: &str, id: String) -> Result<(), ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.call_reply(server, Request::WorkbenchClose { id: id.clone() })
            .await?
            .workbench_close()
            .map_err(ApiError::from)?;
        self.inner.clear_workbench(server, &id);
        Ok(())
    }

    pub async fn workbench_attach(
        &self,
        owner: &str,
        server: &str,
        id: String,
        mode: AttachMode,
        attachment_id: String,
    ) -> Result<Option<WorkbenchAttached>, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.inner.reconcile(server).await?;
        if self.workbench_owner(server, &id, owner).is_some() {
            return Ok(None);
        }
        let previous = self
            .inner
            .leases
            .lock()
            .get(&(owner.to_owned(), server.to_owned()))
            .cloned();
        if let Some(previous) = previous.as_ref().filter(|previous| previous.id != id) {
            self.detach_locked(
                owner,
                server,
                previous.id.clone(),
                previous.attachment_id.clone(),
            )
            .await?;
        }
        let client = self.inner.client(server).await?;
        let request = Request::WorkbenchAttach {
            id: id.clone(),
            mode,
            attachment_id: attachment_id.clone(),
            client: gethostname::gethostname().to_string_lossy().into_owned(),
        };
        let attached = match client.call(&request).await {
            Ok(reply) => reply.workbench_attach().map_err(ApiError::from),
            Err(error)
                if !client.is_closed()
                    && matches!(error, RemoteError::Timeout(_) | RemoteError::Transport(_)) =>
            {
                // A lost response says nothing about admission. Recover only
                // this exact operation on this connection; never replay it.
                self.inner
                    .recover_workbench(server, &client, id.clone(), attachment_id)
                    .await
                    .and_then(|result| {
                        result.ok_or_else(|| {
                            ApiError::Remote(format!("{server}: attach result no longer available"))
                        })
                    })
            }
            Err(error) => {
                if client.is_closed() || matches!(error, RemoteError::Connection(_)) {
                    self.inner.evict(server, &client).await;
                }
                Err(remote_error(server, error))
            }
        };
        if let Ok(WorkbenchAttached::Ready {
            controller_token,
            attachment_id,
            ..
        }) = &attached
        {
            self.inner.leases.lock().insert(
                (owner.to_owned(), server.to_owned()),
                WorkbenchLease {
                    id: id.clone(),
                    attachment_id: attachment_id.clone(),
                    controller_token: controller_token.clone(),
                },
            );
            self.inner.drain_pending_stops();
            let slot = self.inner.slot(server);
            self.inner.ensure_events(server, &slot);
        } else if matches!(
            &attached,
            Ok(WorkbenchAttached::Busy { .. } | WorkbenchAttached::Revoked { .. })
        ) {
            if let Some(previous) = previous.filter(|lease| lease.id == id) {
                self.inner
                    .clear_lease(owner, server, &id, &previous.attachment_id);
            }
        }
        if let Err(error) = self.inner.reconcile(server).await {
            tracing::warn!(%server, %error, "workbench reconciliation after attach failed");
        }
        attached.map(Some)
    }

    pub fn workbench_owned(
        &self,
        owner: &str,
        server: &str,
        id: &str,
        attachment_id: &str,
    ) -> bool {
        self.inner
            .leases
            .lock()
            .get(&(owner.to_owned(), server.to_owned()))
            .is_some_and(|lease| lease.id == id && lease.attachment_id == attachment_id)
    }
    fn workbench_owner(&self, server: &str, id: &str, owner: &str) -> Option<String> {
        self.inner
            .leases
            .lock()
            .iter()
            .find(|((window, remote), lease)| {
                window.as_str() != owner && remote.as_str() == server && lease.id == id
            })
            .map(|((window, _), _)| window.clone())
    }

    pub async fn workbench_detach(
        &self,
        owner: &str,
        server: &str,
        id: String,
        attachment_id: String,
    ) -> Result<(), ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.detach_locked(owner, server, id, attachment_id).await
    }

    async fn detach_locked(
        &self,
        owner: &str,
        server: &str,
        id: String,
        attachment_id: String,
    ) -> Result<(), ApiError> {
        if !self
            .inner
            .leases
            .lock()
            .get(&(owner.to_owned(), server.to_owned()))
            .is_some_and(|lease| lease.id == id && lease.attachment_id == attachment_id)
        {
            return Ok(());
        }
        self.call_reply(
            server,
            Request::WorkbenchDetach {
                id: id.clone(),
                attachment_id: attachment_id.clone(),
            },
        )
        .await?
        .workbench_detach()
        .map_err(ApiError::from)?;
        self.inner.clear_lease(owner, server, &id, &attachment_id);
        Ok(())
    }

    pub async fn workbench_transfer(
        &self,
        source_owner: &str,
        target_owner: &str,
        server: &str,
        id: String,
        attachment_id: String,
    ) -> Result<WorkbenchAttached, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.inner.reconcile(server).await?;
        let source = (source_owner.to_owned(), server.to_owned());
        let target = (target_owner.to_owned(), server.to_owned());
        let lease = {
            let leases = self.inner.leases.lock();
            if leases.contains_key(&target) {
                return Err(ApiError::InvalidArgument(
                    "Target window already owns a workbench on this remote".into(),
                ));
            }
            leases
                .get(&source)
                .filter(|lease| lease.id == id && lease.attachment_id == attachment_id)
                .cloned()
                .ok_or_else(|| not_controller(&id))?
        };
        let client = self.inner.client(server).await?;
        let attached = self
            .inner
            .recover_workbench(server, &client, id.clone(), attachment_id.clone())
            .await?
            .ok_or_else(|| not_controller(&id))?;
        if !matches!(&attached, WorkbenchAttached::Ready { controller_token, attachment_id: current, .. }
            if controller_token == &lease.controller_token && current == &attachment_id)
        {
            return Err(not_controller(&id));
        }
        self.inner.remote_runs.transfer_workbench(
            &self.inner.host,
            source_owner,
            target_owner,
            server,
            &id,
        )?;
        let mut leases = self.inner.leases.lock();
        leases.remove(&source);
        leases.insert(target, lease);
        Ok(attached)
    }

    /// Coordinator calls only after commit or rollback has reached its terminal
    /// owner, so observers cannot discard source state needed for rollback.
    pub fn notify_workbenches_changed(&self, server: &str) {
        if let Err(error) = self.emit(HostEvent::RemoteWorkbenchesChanged {
            server: server.to_owned(),
        }) {
            tracing::warn!(%server, %error, "local workbench transfer notification failed");
        }
    }

    pub async fn workbench_save(
        &self,
        server: &str,
        id: String,
        snapshot: String,
    ) -> Result<(), ApiError> {
        self.call_reply(server, Request::WorkbenchSave { id, snapshot })
            .await?
            .workbench_save()
            .map_err(ApiError::from)
    }

    pub fn release_workbench_owner(&self, owner: &str) {
        let leases: Vec<_> = self
            .inner
            .leases
            .lock()
            .iter()
            .filter(|((window, _), _)| window == owner)
            .map(|((_, server), lease)| (server.clone(), lease.clone()))
            .collect();
        for (server, lease) in leases {
            let router = self.clone();
            let owner = owner.to_owned();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = router
                    .workbench_detach(&owner, &server, lease.id, lease.attachment_id)
                    .await
                {
                    tracing::warn!(%server, %error, "workbench detach on window close failed");
                }
            });
        }
    }

    pub fn remote_runs_release(&self, owner: &str, run_ids: &[String]) -> Result<(), ApiError> {
        self.inner
            .remote_runs
            .release(&self.inner.host, owner, run_ids)
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
        let Some(info) = self.inner.remote_runs.begin_stop(&run_id) else {
            return match local_kind {
                RemoteRunKind::Session => self.inner.host.session_stop(run_id).await,
                RemoteRunKind::Task => self.inner.host.tasks_stop(run_id).await,
            };
        };
        let (remote_result, local_result) = tokio::join!(
            self.inner
                .stop_backend_on(&info.server, &info.workbench, &run_id, info.kind),
            async {
                match info.kind {
                    RemoteRunKind::Session => self.inner.host.session_stop(run_id.clone()).await,
                    RemoteRunKind::Task => self.inner.host.tasks_stop(run_id.clone()).await,
                }
            }
        );
        self.inner.remote_runs.cancel(&run_id, info.generation);
        remote_result.and(local_result)
    }

    async fn local_run_status(&self, run_id: String) -> Result<RunStatus, ApiError> {
        let host = Arc::clone(&self.inner.host);
        tokio::task::spawn_blocking(move || host.run_status(&run_id))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
    }
}

impl RouterInner {
    /// Loaded once; `~/.config/sworm/client.pem` may be a provisioned secret
    /// linked into place, like `~/.ssh/id_ed25519`.
    async fn client_identity(&self) -> Result<Arc<Identity>, ApiError> {
        self.identity
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
            .await
            .map(Arc::clone)
    }

    pub(crate) fn run_status(&self, run_id: &str, state: &str) {
        let _ = (self.events)(HostEvent::RemoteRunStatus {
            run_id: run_id.into(),
            state: state.into(),
        });
    }

    fn set_status(&self, server: &str, status: RemoteStatus) {
        let slot = self.slot(server);
        let mut current = slot.status.lock();
        if *current == status {
            return;
        }
        *current = status.clone();
        drop(current);
        let _ = (self.events)(HostEvent::RemoteStatus {
            server: server.into(),
            connected: status.connected,
            last_error: status.last_error,
            state: status.state,
        });
    }
    fn slot(&self, server: &str) -> Arc<RemoteSlot> {
        Arc::clone(
            self.remotes
                .lock()
                .entry(server.to_owned())
                .or_insert_with(|| Arc::new(RemoteSlot::new())),
        )
    }

    fn transition(&self, server: &str) -> Arc<AsyncMutex<()>> {
        Arc::clone(
            self.transitions
                .lock()
                .entry(server.to_owned())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    /// Caller holds this server's transition gate. Failed observations never
    /// revoke local authority; only a successful list can prove it is gone.
    async fn reconcile(&self, server: &str) -> Result<Vec<WorkbenchInfo>, ApiError> {
        let rows = self
            .call_reply(server, Request::WorkbenchList {})
            .await?
            .workbench_list()
            .map_err(ApiError::from)?;
        let mut leases = self.leases.lock();
        leases.retain(|(_, remote), lease| {
            remote != server || rows.iter().any(|row| row.id == lease.id && row.yours)
        });
        let mut pending = self.pending_stops.lock();
        let before = pending.len();
        pending.retain(|_, stop| {
            stop.server != server
                || rows.iter().any(|row| {
                    row.id == stop.workbench
                        && ((!row.connected && !row.yours) || stop_is_current(&leases, stop))
                })
        });
        if pending.len() != before {
            self.persist_pending_stops(&pending);
        }
        drop(pending);
        self.stop_events_without_leases(server, leases);
        Ok(rows)
    }

    async fn call_reply(&self, server: &str, request: Request) -> Result<Reply, ApiError> {
        self.call_reply_for(server, None, request).await
    }

    async fn recover_workbench(
        &self,
        server: &str,
        client: &Arc<RemoteClient>,
        id: String,
        attachment_id: String,
    ) -> Result<Option<WorkbenchAttached>, ApiError> {
        match client
            .call(&Request::WorkbenchRecover { id, attachment_id })
            .await
        {
            Ok(reply) => reply.workbench_recover().map_err(ApiError::from),
            Err(error) => {
                if !matches!(&error, RemoteError::Wire(_)) {
                    // Failed recovery leaves admission unknown. Retiring this
                    // exact connection prevents untracked live control.
                    client.close();
                    self.evict(server, client).await;
                }
                Err(remote_error(server, error))
            }
        }
    }

    async fn call_reply_for(
        &self,
        server: &str,
        workbench: Option<&str>,
        request: Request,
    ) -> Result<Reply, ApiError> {
        let client = self.client(server).await?;
        let result = match workbench {
            Some(id) => client.call_workbench(id, &request).await,
            None => client.call(&request).await,
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) if matches!(&error, RemoteError::Connection(_)) || client.is_closed() => {
                self.evict(server, &client).await
            }
            Err(error) => return Err(remote_error(server, error)),
        }

        let client = self.client(server).await?;
        let result = match workbench {
            Some(id) => client.call_workbench(id, &request).await,
            None => client.call(&request).await,
        };
        if matches!(&result, Err(RemoteError::Connection(_))) || client.is_closed() {
            self.evict(server, &client).await;
        }
        result.map_err(|error| remote_error(server, error))
    }

    fn clear_workbench(&self, server: &str, id: &str) {
        let mut leases = self.leases.lock();
        leases.retain(|(_, lease_server), lease| lease_server != server || lease.id != id);
        self.drop_unleased_stops(server, id, &leases);
        self.stop_events_without_leases(server, leases);
    }

    fn clear_lease(&self, owner: &str, server: &str, id: &str, attachment_id: &str) {
        let mut leases = self.leases.lock();
        let key = (owner.to_owned(), server.to_owned());
        if leases
            .get(&key)
            .is_some_and(|current| current.id == id && current.attachment_id == attachment_id)
        {
            leases.remove(&key);
            self.drop_unleased_stops(server, id, &leases);
        }
        self.stop_events_without_leases(server, leases);
    }

    fn drop_unleased_stops(
        &self,
        server: &str,
        id: &str,
        leases: &HashMap<(String, String), WorkbenchLease>,
    ) {
        if is_leased(leases, server, id) {
            return;
        }
        let mut pending = self.pending_stops.lock();
        let before = pending.len();
        // Runs remain listed in the workbench after this desktop releases its lease.
        pending.retain(|_, stop| stop.server != server || stop.workbench != id);
        if pending.len() != before {
            self.persist_pending_stops(&pending);
        }
    }

    fn stop_events_without_leases(
        &self,
        server: &str,
        leases: parking_lot::MutexGuard<'_, HashMap<(String, String), WorkbenchLease>>,
    ) {
        let idle = !leases
            .keys()
            .any(|(_, lease_server)| lease_server == server);
        drop(leases);
        if idle {
            if let Some(slot) = self.remotes.lock().get(server).cloned() {
                if slot.claims.lock().is_empty() {
                    slot.stop_events();
                }
            }
        }
    }

    pub(crate) async fn stop_backend(
        self: &Arc<Self>,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let (server, workbench) = self
            .remote_runs
            .target_for(run_id)
            .ok_or_else(|| ApiError::NotFound(format!("Unknown remote run `{run_id}`")))?;
        self.stop_backend_on(&server, &workbench, run_id, kind)
            .await
    }

    async fn stop_backend_on(
        self: &Arc<Self>,
        server: &str,
        workbench: &str,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let transition = self.transition(server);
        let _guard = transition.lock().await;
        let result = async {
            self.reconcile(server).await?;
            if !is_leased(&self.leases.lock(), server, workbench) {
                return Err(not_controller(workbench));
            }
            self.stop_backend_locked(server, workbench, run_id, kind)
                .await
        }
        .await;
        if let Err(error) = &result {
            // Capture authority before releasing the transition gate. A later
            // takeover must not relabel this old stop with its new token.
            self.remember_failed_stop(server, workbench, run_id, kind, error);
        }
        result
    }

    async fn stop_backend_locked(
        &self,
        server: &str,
        workbench: &str,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let request = kind.stop_request(run_id.to_owned());
        let reply = self
            .call_reply_for(server, Some(workbench), request)
            .await?;
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
        workbench: &str,
        run_id: &str,
        kind: RemoteRunKind,
        error: &ApiError,
    ) {
        if matches!(error, ApiError::NotFound(_)) {
            // Neither the run nor the server exists any more: nothing to kill.
            return;
        }
        let leases = self.leases.lock();
        let Some(lease) = leases.iter().find_map(|((_, remote), lease)| {
            (remote == server && lease.id == workbench).then_some(lease)
        }) else {
            return;
        };
        let mut pending = self.pending_stops.lock();
        pending.insert(
            run_id.to_owned(),
            PendingStop {
                workbench: workbench.to_owned(),
                server: server.to_owned(),
                run_id: run_id.to_owned(),
                kind,
                controller_token: lease.controller_token.clone(),
            },
        );
        self.persist_pending_stops(&pending);
        drop(leases);
        drop(pending);
        self.drain_pending_stops();
    }

    /// Forget `stop` unless a rename re-pointed it at another server while it
    /// was in flight: the old name failing says nothing about the run.
    fn forget_pending_stop(&self, stop: &PendingStop) {
        let mut pending = self.pending_stops.lock();
        if pending.get(&stop.run_id).is_some_and(|current| {
            current.server == stop.server && current.controller_token == stop.controller_token
        }) {
            pending.remove(&stop.run_id);
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
        let servers: HashSet<String> = {
            let leases = self.leases.lock();
            self.pending_stops
                .lock()
                .values()
                .filter(|stop| stop_is_current(&leases, stop))
                .map(|stop| stop.server.clone())
                .collect()
        };
        let mut running = self.pending_stop_running.lock();
        for server in servers {
            if !running.insert(server.clone()) {
                continue;
            }
            let router = Arc::downgrade(self);
            // Tauri's runtime: the first drain runs from setup, outside tokio.
            tauri::async_runtime::spawn(async move {
                retry_pending_stops(router, server).await;
            });
        }
    }

    pub(crate) async fn client(&self, server: &str) -> Result<Arc<RemoteClient>, ApiError> {
        let result = self.connect_client(server).await;
        if let Err(error) = &result {
            self.set_status(
                server,
                RemoteStatus {
                    connected: false,
                    last_error: Some(error.to_string()),
                    state: "error".into(),
                },
            );
        }
        result
    }

    async fn connect_client(&self, server: &str) -> Result<Arc<RemoteClient>, ApiError> {
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
        self.set_status(
            server,
            RemoteStatus {
                connected: false,
                last_error: None,
                state: "reconnecting".into(),
            },
        );

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
        let identity = self.client_identity().await?;
        let client = connect_happy(
            &self.endpoint,
            interleave_addresses(addresses),
            identity,
            fingerprint,
        )
        .await
        .map(Arc::new)
        .map_err(|error| remote_error(server, error))?;
        *cached = Some(CachedRemote {
            config,
            client: Arc::clone(&client),
        });
        self.observe_client(server, &slot, &client);
        Ok(client)
    }

    fn observe_client(&self, server: &str, slot: &Arc<RemoteSlot>, client: &Arc<RemoteClient>) {
        self.set_status(
            server,
            RemoteStatus {
                connected: true,
                last_error: None,
                state: "connected".into(),
            },
        );
        let observed = Arc::clone(&client);
        let weak_slot = Arc::downgrade(&slot);
        let events = Arc::clone(&self.events);
        let server = server.to_owned();
        tokio::spawn(async move {
            let reason = observed.closed().await.to_string();
            let Some(slot) = weak_slot.upgrade() else {
                return;
            };
            let cached = slot.cached.lock().await;
            if !cached
                .as_ref()
                .is_some_and(|cached| Arc::ptr_eq(&cached.client, &observed))
            {
                return;
            }
            let status = RemoteStatus {
                connected: false,
                last_error: Some(reason),
                state: "error".into(),
            };
            *slot.status.lock() = status.clone();
            let _ = events(HostEvent::RemoteStatus {
                server,
                connected: false,
                last_error: status.last_error,
                state: status.state,
            });
        });
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
                    if expected.is_some() {
                        self.set_status(&server, RemoteStatus::default());
                    }
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
                self.set_status(
                    server,
                    RemoteStatus {
                        connected: false,
                        last_error: Some("Remote connection lost".into()),
                        state: "reconnecting".into(),
                    },
                );
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
                self.inner.remote_runs.validate_start(
                    &self.inner.host, &$run_id, server, RemoteRunKind::Session,
                    owner_id.as_deref(),
                )?;
                self.inner.remember_claim(server, path, None, false);
                let (reply, workbench, _guard) = self
                    .call_run_reply(
                        server,
                        owner_id.as_deref(),
                        Request::SessionStart {
                            run_id: $run_id.clone(),
                            folder_path: path.to_owned(),
                            provider_id: $provider_id,
                            resume_token: $resume_token,
                            cols: $cols,
                            rows: $rows,
                        },
                    )
                    .await?;
                let result = reply.$method().map_err(ApiError::from)?;
                self.inner.remote_runs.adopt(
                    Arc::downgrade(&self.inner),
                    &self.inner.host,
                    $run_id.clone(),
                    server.to_owned(),
                    workbench,
                    RemoteRunKind::Session,
                    output,
                    events,
                    owner_id,
                )?;
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
                    Some(PtySubscriber { output, events }),
                    owner_id,
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
            $rows:ident: $rows_type:ty,
            $attach_only:ident: $attach_only_type:ty $(,)?
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
            $attach_only: $attach_only_type,
            output: EventSink<Vec<u8>>,
            events: EventSink<PtyEvent>,
            owner_id: Option<String>,
        ) -> Result<$return_type, ApiError> {
            if let Target::Remote { server, path } = Target::parse(&$folder_path)? {
                self.inner.remote_runs.validate_start(
                    &self.inner.host, &$run_id, server, RemoteRunKind::Task,
                    owner_id.as_deref(),
                )?;
                self.inner.remember_claim(server, path, None, false);
                let (reply, workbench, _guard) = self
                    .call_run_reply(
                        server,
                        owner_id.as_deref(),
                        Request::TasksStart {
                            run_id: $run_id.clone(),
                            folder_path: path.to_owned(),
                            task_id: $task_id,
                            active_file_path: $active_file_path,
                            cols: $cols,
                            rows: $rows,
                            attach_only: $attach_only,
                        },
                    )
                    .await?;
                let result = reply.$method().map_err(ApiError::from)?;
                self.inner.remote_runs.adopt(
                    Arc::downgrade(&self.inner),
                    &self.inner.host,
                    $run_id.clone(),
                    server.to_owned(),
                    workbench,
                    RemoteRunKind::Task,
                    output,
                    events,
                    owner_id,
                )?;
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
                    Some(PtySubscriber { output, events }),
                    owner_id,
                    $attach_only,
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
                return self.local_run_status($run_id).await;
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
        #[route(none)]
        AppRuntimeInfo => $method:ident() -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            name: String,
            version: String,
        ) -> Result<$return_type, ApiError> {
            self.inner.host.$method(name, version).await
        }
    };
    (
        #[route(folder_path)]
        FolderClaim => $method:ident($folder_path:ident: $folder_path_type:ty $(,)?) -> $return_type:ty;
    ) => {};
    (
        #[route(folder_path)]
        FolderRelease => $method:ident($folder_path:ident: $folder_path_type:ty $(,)?) -> $return_type:ty;
    ) => {};
    // Workbench operations use required server names and a window-scoped lease.
    (#[route(server)] WorkbenchList => $($rest:tt)*) => {};
    (#[route(server)] WorkbenchClose => $($rest:tt)*) => {};
    (#[route(server)] WorkbenchAttach => $($rest:tt)*) => {};
    (#[route(server)] WorkbenchRecover => $($rest:tt)*) => {};
    (#[route(server)] WorkbenchDetach => $($rest:tt)*) => {};
    (#[route(server)] WorkbenchSave => $($rest:tt)*) => {};
    (
        #[route(none)]
        FolderPathRoot => $method:ident($path:ident: $path_type:ty $(,)?) -> $return_type:ty;
    ) => {
        pub async fn $method(&self, $path: $path_type) -> Result<$return_type, ApiError> {
            reject_remote("folder_path_root", &$path)?;
            self.inner.host.$method($path).await
        }
    };
    (
        #[route(none)]
        OmpResolveUri => $method:ident(
            $uri:ident: $uri_type:ty,
            $cwd:ident: $cwd_type:ty $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $uri: $uri_type,
            $cwd: $cwd_type,
        ) -> Result<$return_type, ApiError> {
            if let Some(cwd) = &$cwd {
                reject_remote("omp_resolve_uri", cwd)?;
            }
            self.inner.host.$method($uri, $cwd).await
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
        #[route(none)]
        $variant:ident => $method:ident(
            $($argument:ident: $argument_type:ty),* $(,)?
        ) -> $return_type:ty;
    ) => {
        pub async fn $method(
            &self,
            $($argument: $argument_type),*
        ) -> Result<$return_type, ApiError> {
            self.inner.host.$method($($argument),*).await
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

/// Each server retries sequentially with its own backoff; an unavailable
/// daemon cannot delay stops headed to another server.
async fn retry_pending_stops(router: Weak<RouterInner>, server: String) {
    let mut delay = INITIAL_RECONNECT_DELAY;
    loop {
        let Some(inner) = router.upgrade() else {
            return;
        };
        let entries: Vec<PendingStop> = {
            let leases = inner.leases.lock();
            inner
                .pending_stops
                .lock()
                .values()
                .filter(|stop| stop.server == server && stop_is_current(&leases, stop))
                .cloned()
                .collect()
        };
        if entries.is_empty() {
            inner.pending_stop_running.lock().remove(&server);
            inner.drain_pending_stops();
            return;
        }
        let mut failed = false;
        for entry in entries {
            let transition = inner.transition(&server);
            let _guard = transition.lock().await;
            if let Err(error) = inner.reconcile(&server).await {
                failed = true;
                tracing::warn!(%server, %error, "pending stop reconciliation failed");
                continue;
            }
            if !stop_is_current(&inner.leases.lock(), &entry) {
                inner.forget_pending_stop(&entry);
                continue;
            }
            match inner
                .stop_backend_locked(&server, &entry.workbench, &entry.run_id, entry.kind)
                .await
            {
                Ok(()) | Err(ApiError::NotFound(_)) => inner.forget_pending_stop(&entry),
                Err(error) => {
                    failed = true;
                    tracing::warn!(%server, run_id = entry.run_id, %error, "retrying remote stop");
                }
            }
        }
        drop(inner);
        if failed {
            sleep(delay).await;
            delay = (delay * 2).min(MAX_RECONNECT_DELAY);
        } else {
            delay = INITIAL_RECONNECT_DELAY;
        }
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
        {
            let transition = router_now.transition(&server);
            let _guard = transition.lock().await;
            if let Err(error) = router_now.reconcile(&server).await {
                tracing::warn!(%server, %error, "workbench reconciliation after reconnect failed");
            }
        }
        reconnect_delay = INITIAL_RECONNECT_DELAY;
        drop(router_now);
        drop(slot_now);

        loop {
            tokio::select! {
                frame = read_frame::<HostEventFrame>(&mut recv) => match frame {
                    Ok(HostEventFrame(event)) => {
                        if let Some(event) = remote_host_event(&server, event) {
                            let Some(router_now) = router.upgrade() else { return };
                            if matches!(&event, HostEvent::RemoteWorkbenchesChanged { .. }) {
                                let transition = router_now.transition(&server);
                                let _guard = transition.lock().await;
                                if let Err(error) = router_now.reconcile(&server).await {
                                    tracing::warn!(%server, %error, "workbench change reconciliation failed");
                                }
                            }
                            if let Err(error) = (router_now.events)(event) {
                                tracing::warn!(%server, %error, "remote host event delivery failed");
                            }
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

fn remote_host_event(server: &str, event: HostEventWire) -> Option<HostEvent> {
    Some(match event {
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
        HostEventWire::WorkbenchesChanged(()) => HostEvent::RemoteWorkbenchesChanged {
            server: server.into(),
        },
        HostEventWire::RecentFoldersChanged(_) => return None,
    })
}

fn resolve_remote_configs() -> Result<HashMap<String, RemoteSettings>, ApiError> {
    let resolved = resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
    Ok(resolved.settings.remotes.into_iter().collect())
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

fn is_leased(
    leases: &HashMap<(String, String), WorkbenchLease>,
    server: &str,
    workbench: &str,
) -> bool {
    leases
        .iter()
        .any(|((_, lease_server), lease)| lease_server == server && lease.id == workbench)
}

fn stop_is_current(leases: &HashMap<(String, String), WorkbenchLease>, stop: &PendingStop) -> bool {
    leases.iter().any(|((_, server), lease)| {
        server == &stop.server
            && lease.id == stop.workbench
            && lease.controller_token == stop.controller_token
    })
}

fn not_controller(workbench: &str) -> ApiError {
    ApiError::from(sworm_protocol::rpc::WireError::NotController {
        workbench: workbench.to_owned(),
    })
}

pub(crate) fn remote_error(server: &str, error: RemoteError) -> ApiError {
    match error {
        RemoteError::Wire(error) => ApiError::from(error),
        RemoteError::Connection(message)
        | RemoteError::Timeout(message)
        | RemoteError::Transport(message)
        | RemoteError::Identity(message) => ApiError::Remote(format!("{server}: {message}")),
    }
}
