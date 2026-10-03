use super::{
    target::{remote_error, Target},
    RouterInner, WorkspaceRouter,
};
use crate::host_events::DesktopEvent;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    str::FromStr,
    sync::{Arc, Weak},
    time::Duration,
};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    services::{
        settings::SettingsService, settings_resolution::resolve_effective_settings_for_folder_path,
    },
};
use sworm_protocol::{
    rpc::{HostEventWire, Open, Reply, Request},
    settings::{EffectiveSettingsPayload, RemoteSettings, SettingsDiagnostic, SettingsOrigin},
};
use sworm_remote::{wire::read_frame, Fingerprint, Identity, RemoteClient};
use tokio::{sync::Mutex as AsyncMutex, task::JoinHandle, time::sleep};

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

pub(super) struct CachedRemote {
    pub(super) config: RemoteSettings,
    pub(super) client: Arc<RemoteClient>,
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
pub(super) struct FolderClaim {
    pub(super) dirs: Option<Vec<String>>,
    pub(super) git: bool,
}

pub(super) struct RemoteSlot {
    pub(super) cached: AsyncMutex<Option<CachedRemote>>,
    pub(super) claims: Mutex<HashMap<String, FolderClaim>>,
    pub(super) events: Mutex<Option<JoinHandle<()>>>,
    pub(super) status: Mutex<RemoteStatus>,
}

impl RemoteSlot {
    pub(super) fn new() -> Self {
        Self {
            cached: AsyncMutex::new(None),
            claims: Mutex::new(HashMap::new()),
            events: Mutex::new(None),
            status: Mutex::new(RemoteStatus::default()),
        }
    }

    pub(super) fn stop_events(&self) {
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

pub(super) struct SettingsCache {
    pub(super) generation: u64,
    pub(super) remotes: HashMap<String, RemoteSettings>,
}

impl WorkspaceRouter {
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
                self.inner
                    .set_status(server, RemoteStatus::error(error.to_string()));
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
        let link = link
            .parse::<sworm_protocol::pairing::PairLink>()
            .map_err(|error| ApiError::InvalidArgument(error.to_string()))?;
        let _guard = self.inner.remote_management.lock().await;
        let remotes = tokio::task::spawn_blocking(resolve_remote_configs).await??;
        if remotes.contains_key(name) != replace {
            return Err(ApiError::InvalidArgument(if replace {
                format!("Remote `{name}` no longer exists; use Pair instead")
            } else {
                format!("Remote `{name}` already exists; use Re-pair instead")
            }));
        }
        let identity = self.inner.client_identity().await?;
        let fingerprint = Fingerprint::from_str(&link.fingerprint)
            .map_err(|error| ApiError::InvalidArgument(error.to_string()))?;
        let client = RemoteClient::connect_host(
            &self.inner.endpoint,
            &link.address(),
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
        let saved_name = name.to_owned();
        let saved_entry =
            serde_json::to_value(&entry).map_err(|error| ApiError::Internal(error.to_string()))?;
        let persisted = self
            .local(move |host| {
                host.settings_update_global_section("remotes", |value| {
                    let mut entries = remote_entries(value)?;
                    if entries.get(&saved_name) != expected.as_ref() {
                        return Err(ApiError::InvalidArgument(
                            "Remote settings changed during pairing; retry".into(),
                        ));
                    }
                    entries.insert(saved_name, saved_entry);
                    Ok(serde_json::Value::Object(entries))
                })
            })
            .await;
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

    pub async fn change_remote(&self, server: &str, name: Option<&str>) -> Result<(), ApiError> {
        if let Some(name) = name {
            validate_server_name(name)?;
        }
        let _guard = self.inner.remote_management.lock().await;
        let inner = Arc::clone(&self.inner);
        let server = server.to_owned();
        let name = name.map(str::to_owned);
        self.local(move |host| {
            // Held across the settings write: the retry loop resolves a stop's
            // server by name, and an unknown name reads as "run gone".
            let mut pending = inner.pending_stops.map.lock();
            if name.is_none() && pending.values().any(|stop| stop.server == server) {
                return Err(ApiError::InvalidArgument(
                    "Reconnect this remote so its pending stops land before removing it".into(),
                ));
            }
            host.settings_update_global_section("remotes", |value| {
                let mut entries = remote_entries(value)?;
                if name.as_ref().is_some_and(|name| entries.contains_key(name)) {
                    return Err(ApiError::InvalidArgument(
                        "Remote name already exists".into(),
                    ));
                }
                let entry = entries.remove(&server).ok_or_else(|| {
                    ApiError::NotFound(format!("Unknown remote server `{server}`"))
                })?;
                if let Some(name) = &name {
                    entries.insert(name.clone(), entry);
                }
                Ok(serde_json::Value::Object(entries))
            })?;
            if let Some(name) = &name {
                let mut moved = false;
                for stop in pending.values_mut().filter(|stop| stop.server == server) {
                    stop.server = name.to_owned();
                    moved = true;
                }
                if moved {
                    inner.pending_stops.persist(host, &pending);
                }
            }
            Ok(())
        })
        .await?;
        self.inner.refresh_settings().await
    }
}

impl RouterInner {
    /// Loaded once; `~/.config/sworm/client.pem` may be a provisioned secret
    /// linked into place, like `~/.ssh/id_ed25519`.
    pub(super) async fn client_identity(&self) -> Result<Arc<Identity>, ApiError> {
        self.identity
            .get_or_try_init(|| async {
                tokio::task::spawn_blocking(|| {
                    let dir = SettingsService::global_config_dir().map_err(ApiError::Internal)?;
                    Identity::load_or_generate(&dir, "client")
                        .map(Arc::new)
                        .map_err(|error| ApiError::Remote(format!("client identity: {error}")))
                })
                .await?
            })
            .await
            .map(Arc::clone)
    }

    pub(super) fn set_status(&self, server: &str, status: RemoteStatus) {
        self.slot(server).set_status(server, status, &self.events);
    }
    pub(super) fn slot(&self, server: &str) -> Arc<RemoteSlot> {
        Arc::clone(
            self.remotes
                .lock()
                .entry(server.to_owned())
                .or_insert_with(|| Arc::new(RemoteSlot::new())),
        )
    }

    pub(crate) async fn client(&self, server: &str) -> Result<Arc<RemoteClient>, ApiError> {
        let result = self.connect_client(server).await;
        if let Err(error) = &result {
            self.set_status(server, RemoteStatus::error(error.to_string()));
        }
        result
    }

    pub(super) async fn connect_client(&self, server: &str) -> Result<Arc<RemoteClient>, ApiError> {
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
        self.set_status(server, RemoteStatus::reconnecting(None));

        let identity = self.client_identity().await?;
        let client =
            RemoteClient::connect_host(&self.endpoint, &config.address, identity, fingerprint)
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

    pub(super) fn observe_client(
        &self,
        server: &str,
        slot: &Arc<RemoteSlot>,
        client: &Arc<RemoteClient>,
    ) {
        self.set_status(server, RemoteStatus::connected());
        let observed = Arc::clone(client);
        let weak_slot = Arc::downgrade(slot);
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
            slot.set_status(&server, RemoteStatus::error(reason), &events);
        });
    }

    pub(super) async fn refresh_settings(&self) -> Result<(), ApiError> {
        let generation = self.host.settings_generation();
        let mut settings = self.settings.lock().await;
        if settings.generation == generation {
            return Ok(());
        }
        let remotes = tokio::task::spawn_blocking(resolve_remote_configs).await??;
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
                    RemoteStatus::reconnecting(Some("Remote connection lost".into())),
                );
            }
        }
    }

    pub(super) fn remember_claim(
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

    pub(super) fn ensure_events(self: &Arc<Self>, server: &str, slot: &Arc<RemoteSlot>) {
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

async fn run_events(router: Weak<RouterInner>, slot: Weak<RemoteSlot>, server: String) {
    let mut backoff = Backoff::default();
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
                backoff.wait().await;
                continue;
            }
        };
        let Ok((_send, mut recv)) = client.open_stream(Open::Events).await else {
            router_now.evict(&server, &client).await;
            drop(router_now);
            drop(slot_now);
            backoff.wait().await;
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
        backoff.reset();
        drop(router_now);
        drop(slot_now);

        loop {
            tokio::select! {
                frame = read_frame::<HostEventWire>(&mut recv) => match frame {
                    Ok(event) => {
                        if let Some(event) = remote_host_event(&server, event) {
                            let Some(router_now) = router.upgrade() else { return };
                            if matches!(&event, DesktopEvent::WorkbenchesChanged { .. }) {
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
        backoff.wait().await;
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
            .call_as(
                &Request::FolderResolve {
                    path: folder.clone(),
                },
                Reply::folder_resolve,
            )
            .await;
        if let Err(error) = claim_result {
            tracing::warn!(%server, %folder, %error, "remote folder claim restoration failed");
            continue;
        }
        if let Some(dirs) = claim.dirs {
            let result = client
                .call_as(
                    &Request::FilesWatchDirs {
                        project_path: folder.clone(),
                        dirs,
                    },
                    Reply::files_watch_dirs,
                )
                .await;
            if let Err(error) = result {
                tracing::warn!(%server, %folder, %error, "remote file watch restoration failed");
            }
        }
        if claim.git {
            let result = client
                .call_as(
                    &Request::GitWatch {
                        project_path: folder.clone(),
                    },
                    Reply::git_watch,
                )
                .await;
            if let Err(error) = result {
                tracing::warn!(%server, %folder, %error, "remote git watch restoration failed");
            }
        }
    }
}

fn remote_host_event(server: &str, event: HostEventWire) -> Option<DesktopEvent> {
    Some(DesktopEvent::Host(match event {
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
        HostEventWire::WorkbenchesChanged(()) => {
            return Some(DesktopEvent::WorkbenchesChanged {
                server: server.into(),
            })
        }
        HostEventWire::RecentFoldersChanged(_) => return None,
    }))
}

fn resolve_remote_configs() -> Result<HashMap<String, RemoteSettings>, ApiError> {
    let resolved = resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
    Ok(resolved.settings.remotes.into_iter().collect())
}

/// Retag diagnostics resolved by `server`'s daemon: `Host` origin, and paths in
/// `sworm://<server>/…` form so two same-named `settings.jsonc` files on
/// different machines stay distinguishable both to code and in the status bar.
pub(super) fn tag_host_diagnostics(diagnostics: &mut [SettingsDiagnostic], server: &str) {
    for diagnostic in diagnostics {
        diagnostic.origin = SettingsOrigin::Host;
        diagnostic.path = format!(
            "sworm://{server}/{}",
            diagnostic.path.trim_start_matches('/')
        );
    }
}

/// Overlay the desktop's own `DESKTOP_SECTIONS` onto a remote workspace's
/// effective settings: host sections resolve on the machine that runs the
/// folder, window/terminal/remotes describe this window.
///
/// Both machines' diagnostics survive the merge, the daemon's retagged as
/// `Host` so the desktop can tell them apart.
pub(super) fn merge_desktop_sections(
    remote: &mut EffectiveSettingsPayload,
    local: EffectiveSettingsPayload,
    server: &str,
) {
    remote.settings.window = local.settings.window;
    remote.settings.terminal = local.settings.terminal;
    remote.settings.remotes = local.settings.remotes;
    tag_host_diagnostics(&mut remote.diagnostics, server);
    remote.diagnostics.extend(local.diagnostics);
}

impl RemoteStatus {
    pub(super) fn connected() -> Self {
        Self {
            connected: true,
            last_error: None,
            state: "connected".into(),
        }
    }

    pub(super) fn reconnecting(last_error: Option<String>) -> Self {
        Self {
            connected: false,
            last_error,
            state: "reconnecting".into(),
        }
    }

    pub(super) fn error(error: String) -> Self {
        Self {
            connected: false,
            last_error: Some(error),
            state: "error".into(),
        }
    }
}

impl RemoteSlot {
    pub(super) fn set_status(
        &self,
        server: &str,
        status: RemoteStatus,
        events: &EventSink<DesktopEvent>,
    ) {
        let mut current = self.status.lock();
        if *current == status {
            return;
        }
        *current = status.clone();
        drop(current);
        let _ = events(DesktopEvent::RemoteStatus {
            server: server.into(),
            connected: status.connected,
            last_error: status.last_error,
            state: status.state,
        });
    }
}

pub(super) struct Backoff(Duration);

impl Default for Backoff {
    fn default() -> Self {
        Self(Duration::from_secs(1))
    }
}

impl Backoff {
    pub(super) async fn wait(&mut self) {
        sleep(self.0).await;
        self.0 = (self.0 * 2).min(Duration::from_secs(30));
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use sworm_protocol::settings::*;

    #[test]
    fn settings_effective_merges_desktop_sections() {
        let mut remote = EffectiveSettingsPayload {
            settings: EffectiveSettings::default(),
            diagnostics: vec![diagnostic("/srv/repo/.sworm/settings.jsonc", "/explorer")],
        };
        remote.settings.terminal.font_size = 11;
        remote.settings.window.tab_beam_position = TabBeamPosition::Bottom;
        remote.settings.explorer.exclude = BTreeMap::from([("**/target".to_owned(), true)]);
        remote.settings.nix.eval_timeout_secs = 42;

        let mut local = EffectiveSettingsPayload {
            settings: EffectiveSettings::default(),
            diagnostics: vec![diagnostic(
                "/home/me/.config/sworm/settings.jsonc",
                "/window",
            )],
        };
        local.settings.terminal.font_size = 17;
        local.settings.window.tab_beam_position = TabBeamPosition::Top;
        local.settings.remotes = BTreeMap::from([(
            "loop".to_owned(),
            RemoteSettings {
                address: "127.0.0.1:7420".to_owned(),
                fingerprint: "SHA256:beef".to_owned(),
            },
        )]);

        merge_desktop_sections(&mut remote, local, "loop");

        // Desktop sections win, host sections stay on the daemon's values.
        assert_eq!(remote.settings.terminal.font_size, 17);
        assert_eq!(
            remote.settings.window.tab_beam_position,
            TabBeamPosition::Top
        );
        assert!(remote.settings.remotes.contains_key("loop"));
        assert_eq!(
            remote.settings.explorer.exclude,
            BTreeMap::from([("**/target".to_owned(), true)])
        );
        assert_eq!(remote.settings.nix.eval_timeout_secs, 42);

        // Each merged diagnostic says which machine resolved it, keeps its
        // layer, and keeps a path that names the machine too.
        let merged: Vec<_> = remote
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.origin,
                    diagnostic.layer,
                    diagnostic.path.as_str(),
                )
            })
            .collect();
        assert_eq!(
            merged,
            vec![
                (
                    SettingsOrigin::Host,
                    SettingsLayerKind::Folder,
                    "sworm://loop/srv/repo/.sworm/settings.jsonc"
                ),
                (
                    SettingsOrigin::Desktop,
                    SettingsLayerKind::Folder,
                    "/home/me/.config/sworm/settings.jsonc"
                )
            ]
        );
    }

    fn diagnostic(path: &str, pointer: &str) -> SettingsDiagnostic {
        SettingsDiagnostic {
            layer: SettingsLayerKind::Folder,
            origin: SettingsOrigin::Desktop,
            path: path.to_owned(),
            pointer: pointer.to_owned(),
            code: SettingsDiagnosticCode::InvalidValue,
            severity: SettingsDiagnosticSeverity::Warning,
            message: "bad value".to_owned(),
        }
    }
}
