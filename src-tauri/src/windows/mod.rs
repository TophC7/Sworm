use crate::app_state::AppState;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sworm_core::services::app_state_kv::AppStateKvService;
use sworm_core::services::pty::{PtyService, PtySubscriber};
use sworm_core::services::settings_resolution::resolve_effective_settings_for_folder_path;
use sworm_protocol::settings::{ExternalFileOpenMode, ExternalFolderOpenMode, WindowSettings};
use tauri::{
    Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

mod groups;

const MANIFEST_KEY: &str = "window_manifest";
const MANIFEST_VERSION: u32 = 1;
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowManifestEntry {
    pub label: String,
    pub bounds: Option<WindowBounds>,
    pub maximized: bool,
    pub focus_order: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WindowManifest {
    #[serde(default = "default_manifest_version")]
    pub version: u32,
    #[serde(default)]
    pub windows: Vec<WindowManifestEntry>,
}

const fn default_manifest_version() -> u32 {
    MANIFEST_VERSION
}

impl Default for WindowManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            windows: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpenTarget {
    Folder {
        folder_path: String,
    },
    File {
        folder_path: String,
        /// Project-relative to `folder_path`.
        file_path: String,
    },
    Workbench {
        server: String,
        workbench_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ClaimFileResult {
    Claimed,
    Redirect { owner_label: String, tab_id: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabTransferInitiateParams {
    pub source_window: String,
    pub target_window: String,
    pub tab_id: String,
    pub target_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabTransferExportPayload {
    pub transfer_id: String,
    pub tab: serde_json::Value,
    pub terminal_state: Option<serde_json::Value>,
    pub model_state: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
struct ActiveTransfer {
    id: String,
    source_window: String,
    target_window: String,
    tab_id: String,
    target_index: usize,
    created_at: Instant,
    exported: bool,
    pty_run_id: Option<String>,
    file_path: Option<PathBuf>,
}

#[derive(Clone, Debug)]
struct LiveWindowRecord {
    label: String,
    bounds: Option<WindowBounds>,
    maximized: bool,
    focus_order: usize,
    ready: bool,
    pending_open_targets: Vec<OpenTarget>,
    folder_claims: HashSet<PathBuf>,
    file_claims: HashMap<PathBuf, String>,
}

pub struct WindowCoordinatorService {
    records: Mutex<HashMap<String, LiveWindowRecord>>,
    active_transfers: Mutex<HashMap<String, ActiveTransfer>>,
    group_transfers: Mutex<HashMap<String, Arc<groups::GroupTransfer>>>,
    exit_requested: AtomicBool,
    pending_cleanup: Mutex<Vec<std::sync::mpsc::Receiver<()>>>,
}

impl WindowCoordinatorService {
    pub fn new() -> Self {
        Self {
            records: Mutex::new(HashMap::new()),
            active_transfers: Mutex::new(HashMap::new()),
            group_transfers: Mutex::new(HashMap::new()),
            exit_requested: AtomicBool::new(false),
            pending_cleanup: Mutex::new(Vec::new()),
        }
    }

    fn track_cleanup(&self) -> std::sync::mpsc::Sender<()> {
        let (finished, pending) = std::sync::mpsc::channel();
        let mut cleanup = self.pending_cleanup.lock();
        cleanup.retain(|job| matches!(job.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
        cleanup.push(pending);
        finished
    }

    /// Native close cleanup must finish before the final manifest save and Host shutdown.
    pub(crate) fn wait_for_cleanup(&self) {
        let cleanup = std::mem::take(&mut *self.pending_cleanup.lock());
        for job in cleanup {
            let _ = job.recv();
        }
    }

    pub fn load_manifest(conn: &Connection) -> Result<WindowManifest, String> {
        if let Some(json) = AppStateKvService::get(conn, MANIFEST_KEY)? {
            return serde_json::from_str(&json)
                .map_err(|error| format!("window manifest parse failed: {error}"));
        }

        let Some(legacy_json) = AppStateKvService::get(conn, "workbench")? else {
            return Ok(WindowManifest::default());
        };

        let tx = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        let label = format!("workbench-{}", uuid::Uuid::new_v4());
        AppStateKvService::put(&tx, &format!("workbench:{label}"), &legacy_json)?;
        AppStateKvService::delete(&tx, "workbench")?;
        let manifest = WindowManifest {
            version: MANIFEST_VERSION,
            windows: vec![WindowManifestEntry {
                label,
                bounds: None,
                maximized: false,
                focus_order: 0,
            }],
        };
        let json = serde_json::to_string(&manifest)
            .map_err(|error| format!("window manifest serialization failed: {error}"))?;
        AppStateKvService::put(&tx, MANIFEST_KEY, &json)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(manifest)
    }

    pub fn save_manifest(&self, app: &tauri::AppHandle) -> Result<(), String> {
        let state = app
            .try_state::<AppState>()
            .ok_or_else(|| "AppState is not initialized".to_string())?;
        let db = state.host.db.write();
        self.save_manifest_with(db.conn())
    }

    pub fn save_manifest_with(&self, conn: &Connection) -> Result<(), String> {
        let json = serde_json::to_string(&self.current_manifest())
            .map_err(|error| format!("window manifest serialization failed: {error}"))?;
        AppStateKvService::put(conn, MANIFEST_KEY, &json)
    }

    fn current_manifest(&self) -> WindowManifest {
        let mut windows: Vec<_> = self
            .records
            .lock()
            .values()
            .map(|record| WindowManifestEntry {
                label: record.label.clone(),
                bounds: record.bounds.clone(),
                maximized: record.maximized,
                focus_order: record.focus_order,
            })
            .collect();
        windows.sort_by_key(|entry| entry.focus_order);
        WindowManifest {
            version: MANIFEST_VERSION,
            windows,
        }
    }

    pub fn create_workbench_window(
        &self,
        app: &tauri::AppHandle,
        entry: Option<WindowManifestEntry>,
    ) -> Result<WebviewWindow, String> {
        let restoring = entry.is_some();
        let entry = entry.unwrap_or_else(|| WindowManifestEntry {
            label: format!("workbench-{}", uuid::Uuid::new_v4()),
            bounds: None,
            maximized: false,
            focus_order: self.next_focus_order(),
        });
        let label = entry.label.clone();
        let mut valid_bounds = None;
        let mut builder =
            WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
                .title(app.package_info().name.clone())
                .decorations(false)
                .min_inner_size(800.0, 500.0);
        if let Some((bounds, monitor)) = entry.bounds.as_ref().and_then(|bounds| {
            bounds_intersect_monitor(app, bounds).map(|monitor| (bounds, monitor))
        }) {
            let mon_pos = monitor.position();
            let mon_size = monitor.size();
            let width = bounds.width.min(mon_size.width);
            let height = bounds.height.min(mon_size.height);
            let x = bounds
                .x
                .clamp(mon_pos.x, mon_pos.x + mon_size.width as i32 - width as i32);
            let y = bounds.y.clamp(
                mon_pos.y,
                mon_pos.y + mon_size.height as i32 - height as i32,
            );
            let sf = monitor.scale_factor();
            builder = builder
                .position(x as f64 / sf, y as f64 / sf)
                .inner_size(width as f64 / sf, height as f64 / sf);
            valid_bounds = Some(WindowBounds {
                x,
                y,
                width,
                height,
            });
        } else {
            builder = builder.inner_size(1200.0, 800.0);
        }
        let window = builder
            .build()
            .map_err(|error| format!("failed to create window {label}: {error}"))?;
        if entry.maximized {
            window
                .maximize()
                .map_err(|error| format!("failed to maximize window {label}: {error}"))?;
        }

        self.records.lock().insert(
            label.clone(),
            LiveWindowRecord {
                label: label.clone(),
                bounds: valid_bounds,
                maximized: entry.maximized,
                focus_order: entry.focus_order,
                ready: false,
                pending_open_targets: Vec::new(),
                folder_claims: HashSet::new(),
                file_claims: HashMap::new(),
            },
        );

        let event_app = app.clone();
        let event_label = label.clone();
        let event_window = window.clone();
        window.on_window_event(move |event| {
            let Some(state) = event_app.try_state::<AppState>() else {
                return;
            };
            match event {
                WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
                    let minimized = event_window.is_minimized().unwrap_or(false);
                    let maximized = event_window.is_maximized().unwrap_or(false);
                    let mut records = state.windows.records.lock();
                    let Some(record) = records.get_mut(&event_label) else {
                        return;
                    };
                    record.maximized = maximized;
                    if minimized || maximized {
                        return;
                    }
                    if record.bounds.is_none() {
                        let (Ok(position), Ok(size)) =
                            (event_window.outer_position(), event_window.inner_size())
                        else {
                            return;
                        };
                        record.bounds = Some(WindowBounds {
                            x: position.x,
                            y: position.y,
                            width: size.width,
                            height: size.height,
                        });
                    }
                    if let Some(bounds) = record.bounds.as_mut() {
                        match event {
                            WindowEvent::Moved(pos) => {
                                bounds.x = pos.x;
                                bounds.y = pos.y;
                            }
                            WindowEvent::Resized(size) => {
                                bounds.width = size.width;
                                bounds.height = size.height;
                            }
                            _ => {}
                        }
                    }
                }
                WindowEvent::Focused(true) => {
                    state.windows.record_focus(&event_label);
                }
                WindowEvent::Destroyed => {
                    // Runs after the frontend's CloseRequested handler confirmed
                    // and flushed, so a cancelled close never tears anything down.
                    state.windows.cancel_groups_for_window(&event_label);
                    let discard_snapshot = state.windows.destroy_discards_snapshot();
                    let detach_runs = state.windows.destroy_detaches_runs();
                    let finished = state.windows.track_cleanup();
                    let app = event_app.clone();
                    let label = event_label.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        finish_window_close(&app, &label, discard_snapshot, detach_runs);
                        let _ = finished.send(());
                    });
                }
                _ => {}
            }
        });

        if !restoring && app.try_state::<AppState>().is_some() {
            self.save_manifest(app)?;
        }
        Ok(window)
    }

    pub fn mark_ready(
        &self,
        label: &str,
        app: &tauri::AppHandle,
    ) -> Result<Vec<OpenTarget>, String> {
        let targets = {
            let mut records = self.records.lock();
            let record = records
                .get_mut(label)
                .ok_or_else(|| format!("unknown window label: {label}"))?;
            record.ready = true;
            std::mem::take(&mut record.pending_open_targets)
        };
        let window = app
            .get_webview_window(label)
            .ok_or_else(|| format!("window not found: {label}"))?;
        for target in &targets {
            window
                .emit_to(window.label(), "open-target", target)
                .map_err(|error| format!("failed to emit open target to {label}: {error}"))?;
        }
        self.request_ready_groups(app, label);
        crate::deep_links::notify(app);
        Ok(targets)
    }

    pub fn queue_open_target(&self, label: &str, target: OpenTarget, app: &tauri::AppHandle) {
        let ready = {
            let mut records = self.records.lock();
            let Some(record) = records.get_mut(label) else {
                tracing::warn!("Cannot route open target to unknown window {label}");
                return;
            };
            if !record.ready {
                record.pending_open_targets.push(target.clone());
            }
            record.ready
        };
        if ready {
            if let Some(window) = app.get_webview_window(label) {
                if let Err(error) = window.emit_to(window.label(), "open-target", &target) {
                    tracing::error!("Failed to emit open target to {label}: {error}");
                }
            }
        }
    }

    pub fn claim_folder(&self, label: &str, folder: PathBuf) {
        if let Some(record) = self.records.lock().get_mut(label) {
            record.folder_claims.insert(folder);
        }
    }

    pub fn release_folder(&self, label: &str, folder: &Path) -> bool {
        let mut records = self.records.lock();
        if let Some(record) = records.get_mut(label) {
            record.folder_claims.remove(folder);
        }
        !records
            .values()
            .any(|record| record.folder_claims.contains(folder))
    }

    pub fn folder_claimed(&self, folder: &Path) -> bool {
        self.records
            .lock()
            .values()
            .any(|record| record.folder_claims.contains(folder))
    }

    pub fn claim_file(
        &self,
        window: &WebviewWindow,
        file_path: PathBuf,
        tab_id: String,
        reveal: Option<serde_json::Value>,
    ) -> Result<ClaimFileResult, String> {
        let result = self.claim_file_record(window.label(), &file_path, &tab_id);
        let ClaimFileResult::Redirect {
            owner_label,
            tab_id: owner_tab_id,
        } = &result
        else {
            return Ok(result);
        };
        let app = window.app_handle();
        focus_window(app, owner_label);
        if let Some(window) = app.get_webview_window(owner_label) {
            window
                .emit_to(
                    window.label(),
                    "focus-tab",
                    serde_json::json!({ "tabId": owner_tab_id, "reveal": reveal }),
                )
                .map_err(|error| {
                    format!("failed to focus claimed tab in {owner_label}: {error}")
                })?;
        }
        Ok(result)
    }

    fn claim_file_record(&self, label: &str, file: &Path, tab_id: &str) -> ClaimFileResult {
        let mut records = self.records.lock();
        if let Some((owner_label, owner_tab_id)) =
            records.iter().find_map(|(other_label, record)| {
                (other_label != label)
                    .then(|| record.file_claims.get(file))
                    .flatten()
                    .map(|other_tab_id| (other_label.clone(), other_tab_id.clone()))
            })
        {
            return ClaimFileResult::Redirect {
                owner_label,
                tab_id: owner_tab_id,
            };
        }
        if let Some(record) = records.get_mut(label) {
            record
                .file_claims
                .insert(file.to_path_buf(), tab_id.to_string());
        }
        ClaimFileResult::Claimed
    }

    pub fn release_file(&self, label: &str, file: &Path) {
        if let Some(record) = self.records.lock().get_mut(label) {
            record.file_claims.remove(file);
        }
    }

    pub fn release_claims_under(&self, root: &Path) -> usize {
        let mut removed = 0;
        for record in self.records.lock().values_mut() {
            let keys: Vec<_> = record
                .file_claims
                .keys()
                .filter(|key| key.starts_with(root))
                .cloned()
                .collect();
            for key in keys {
                record.file_claims.remove(&key);
                removed += 1;
            }
        }
        removed
    }

    pub fn rename_claims_under(&self, old_root: &Path, new_root: &Path) {
        for record in self.records.lock().values_mut() {
            let keys: Vec<_> = record
                .file_claims
                .keys()
                .filter(|key| key.starts_with(old_root))
                .cloned()
                .collect();
            for key in keys {
                if let Some(tab_id) = record.file_claims.remove(&key) {
                    record
                        .file_claims
                        .insert(new_root.join(key.strip_prefix(old_root).unwrap()), tab_id);
                }
            }
        }
    }

    pub fn transfer_file_claim(
        &self,
        source_label: &str,
        target_label: &str,
        file: &Path,
        tab_id: &str,
    ) -> Result<(), String> {
        let mut records = self.records.lock();
        if !records.contains_key(target_label) {
            return Err(format!("unknown target window label: {target_label}"));
        }
        if source_label != target_label {
            if let Some(source) = records.get_mut(source_label) {
                source.file_claims.remove(file);
            }
        }
        records
            .get_mut(target_label)
            .expect("target window checked above")
            .file_claims
            .insert(file.to_path_buf(), tab_id.to_string());
        Ok(())
    }
    pub fn initiate_tab_transfer<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        caller: &str,
        params: TabTransferInitiateParams,
    ) -> Result<String, String> {
        if caller != params.target_window {
            return Err("transfer target belongs to a different window".to_string());
        }
        self.abort_expired_transfers(app);
        {
            let records = self.records.lock();
            if !records.contains_key(&params.source_window) {
                return Err(format!(
                    "unknown source window label: {}",
                    params.source_window
                ));
            }
            if !records.contains_key(&params.target_window) {
                return Err(format!(
                    "unknown target window label: {}",
                    params.target_window
                ));
            }
        }

        let mut transfers = self.active_transfers.lock();
        if transfers.values().any(|transfer| {
            transfer.source_window == params.source_window && transfer.tab_id == params.tab_id
        }) {
            return Err("tab transfer already in progress for this tab".to_string());
        }
        let transfer_id = format!("transfer-{}", uuid::Uuid::new_v4());
        transfers.insert(
            transfer_id.clone(),
            ActiveTransfer {
                id: transfer_id.clone(),
                source_window: params.source_window.clone(),
                target_window: params.target_window,
                tab_id: params.tab_id.clone(),
                target_index: params.target_index,
                created_at: Instant::now(),
                exported: false,
                pty_run_id: None,
                file_path: None,
            },
        );
        drop(transfers);

        if let Err(error) = app
            .emit_to(
                &params.source_window,
                "tab-transfer-request",
                serde_json::json!({
                    "transferId": &transfer_id,
                    "tabId": &params.tab_id,
                }),
            )
            .map_err(|error| format!("failed to request tab transfer: {error}"))
        {
            self.active_transfers.lock().remove(&transfer_id);
            return Err(error);
        }

        let timeout_app = app.clone();
        let timeout_id = transfer_id.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(TRANSFER_TIMEOUT).await;
            if let Err(error) = tauri::async_runtime::spawn_blocking(move || {
                if let Some(state) = timeout_app.try_state::<AppState>() {
                    state
                        .windows
                        .abort_expired_transfer(&timeout_app, &timeout_id);
                }
            })
            .await
            {
                tracing::error!("Transfer timeout cleanup failed: {error}");
            }
        });
        Ok(transfer_id)
    }

    pub fn source_export_ready<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        source_window: &str,
        payload: TabTransferExportPayload,
    ) -> Result<(), String> {
        self.abort_expired_transfers(app);
        let transfer = self
            .active_transfers
            .lock()
            .get(&payload.transfer_id)
            .cloned()
            .ok_or_else(|| format!("unknown or expired transfer: {}", payload.transfer_id))?;
        if transfer.source_window != source_window {
            return Err("transfer export belongs to a different window".to_string());
        }
        if payload.tab.get("id").and_then(serde_json::Value::as_str)
            != Some(transfer.tab_id.as_str())
        {
            return Err("exported tab does not match transfer request".to_string());
        }

        let kind = payload.tab.get("kind").and_then(serde_json::Value::as_str);
        let pty_run_id = matches!(kind, Some("session" | "task"))
            .then(|| {
                payload
                    .terminal_state
                    .as_ref()
                    .and_then(|state| state.get("runId"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .flatten();
        if let Some(run_id) = &pty_run_id {
            let state = app
                .try_state::<AppState>()
                .ok_or_else(|| "AppState is not initialized".to_string())?;
            state
                .host
                .pty
                .ensure_transfer_owner(run_id, source_window)?;
        }
        let file_path = (kind == Some("text"))
            .then(|| {
                self.records
                    .lock()
                    .get(&transfer.source_window)
                    .and_then(|record| {
                        record
                            .file_claims
                            .iter()
                            .find(|(_, tab_id)| *tab_id == &transfer.tab_id)
                            .map(|(path, _)| path.clone())
                    })
            })
            .flatten();

        {
            let mut transfers = self.active_transfers.lock();
            let active = transfers
                .get_mut(&payload.transfer_id)
                .ok_or_else(|| format!("unknown or expired transfer: {}", payload.transfer_id))?;
            active.exported = true;
            active.pty_run_id = pty_run_id;
            active.file_path = file_path;
        }

        app.emit_to(
            &transfer.target_window,
            "tab-transfer-import",
            serde_json::json!({
                "transferId": &payload.transfer_id,
                "exportPayload": payload,
                "targetIndex": transfer.target_index,
            }),
        )
        .map_err(|error| format!("failed to import tab transfer: {error}"))
    }

    pub fn target_stage_ready<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        target_window: &str,
        transfer_id: &str,
    ) -> Result<(), String> {
        self.abort_expired_transfers(app);
        let mut transfers = self.active_transfers.lock();
        let transfer = transfers
            .get(transfer_id)
            .cloned()
            .ok_or_else(|| format!("unknown or expired transfer: {transfer_id}"))?;
        if transfer.target_window != target_window || !transfer.exported {
            return Err("transfer is not staged for this target window".to_string());
        }

        if let Some(run_id) = &transfer.pty_run_id {
            let state = app
                .try_state::<AppState>()
                .ok_or_else(|| "AppState is not initialized".to_string())?;
            state
                .host
                .pty
                .transfer_owner(run_id, &transfer.target_window)?;
        }
        if let Some(file_path) = &transfer.file_path {
            if let Err(error) = self.transfer_file_claim(
                &transfer.source_window,
                &transfer.target_window,
                file_path,
                &transfer.tab_id,
            ) {
                if let Some(run_id) = &transfer.pty_run_id {
                    if let Some(state) = app.try_state::<AppState>() {
                        let _ = state
                            .host
                            .pty
                            .transfer_owner(run_id, &transfer.source_window);
                    }
                }
                return Err(error);
            }
        }
        if let Some(run_id) = &transfer.pty_run_id {
            if let Some(state) = app.try_state::<AppState>() {
                state.host.pty.commit_transfer(run_id);
            }
        }
        transfers.remove(transfer_id);
        drop(transfers);

        self.emit_transfer_event(
            app,
            &transfer.source_window,
            "tab-transfer-committed",
            serde_json::json!({
                "transferId": &transfer.id,
                "tabId": &transfer.tab_id,
            }),
        );
        self.emit_transfer_event(
            app,
            &transfer.target_window,
            "tab-transfer-finalized",
            serde_json::json!({ "transferId": &transfer.id }),
        );
        Ok(())
    }

    pub fn abort_tab_transfer_as<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        caller: &str,
        transfer_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.authorize_participant(transfer_id, caller)?;
        self.abort_tab_transfer(app, transfer_id, reason);
        Ok(())
    }

    pub fn abort_tab_transfer<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        transfer_id: &str,
        reason: &str,
    ) {
        let transfer = self.active_transfers.lock().remove(transfer_id);
        if let Some(transfer) = transfer {
            let mut pty_lost = false;
            if let Some(run_id) = &transfer.pty_run_id {
                if let Some(app_state) = app.try_state::<AppState>() {
                    if app_state.host.pty.resume_original(run_id).is_err()
                        && app_state.host.pty.abort_transfer_detached(run_id).is_err()
                    {
                        let _ = app_state.host.discard_run(run_id);
                        pty_lost = true;
                    }
                }
            }
            let payload = serde_json::json!({
                "transferId": &transfer.id,
                "reason": reason,
                "ptyLost": pty_lost,
            });
            self.emit_transfer_event(
                app,
                &transfer.source_window,
                "tab-transfer-aborted",
                payload.clone(),
            );
            self.emit_transfer_event(
                app,
                &transfer.target_window,
                "tab-transfer-aborted",
                payload,
            );
        }
    }
    /// Settle transfers touching a window that is going away. A target that
    /// closes aborts; a source that closes before exporting aborts; a source
    /// that already exported leaves the tab to the target.
    pub(crate) fn abort_transfers_for_window<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        label: &str,
    ) {
        let doomed: Vec<(String, &'static str)> = self
            .active_transfers
            .lock()
            .values()
            .filter_map(|transfer| {
                if transfer.target_window == label {
                    Some((transfer.id.clone(), "target window closed"))
                } else if transfer.source_window == label && !transfer.exported {
                    Some((transfer.id.clone(), "source window closed"))
                } else {
                    None
                }
            })
            .collect();
        for (transfer_id, reason) in doomed {
            self.abort_tab_transfer(app, &transfer_id, reason);
        }
    }

    pub fn protected_pty_runs(&self, label: &str) -> HashSet<String> {
        self.active_transfers
            .lock()
            .values()
            .filter(|transfer| transfer.source_window == label && transfer.exported)
            .filter_map(|transfer| transfer.pty_run_id.clone())
            .collect()
    }
    /// Hold the transfer lock through PTY attachment: abort/commit must not
    /// invalidate authorization before the subscriber is installed.
    pub fn attach_transfer_pty(
        &self,
        transfer_id: &str,
        window_label: &str,
        run_id: &str,
        subscriber: PtySubscriber,
        pty: &PtyService,
    ) -> Result<u64, String> {
        let transfers = self.active_transfers.lock();
        let transfer = transfers
            .get(transfer_id)
            .filter(|transfer| {
                transfer.exported
                    && transfer.created_at.elapsed() < TRANSFER_TIMEOUT
                    && transfer.target_window == window_label
                    && transfer.pty_run_id.as_deref() == Some(run_id)
            })
            .ok_or_else(|| "attach is not part of an active transfer".to_string())?;
        pty.ensure_transfer_owner(run_id, &transfer.source_window)?;
        pty.attach(run_id, subscriber)
    }

    fn authorize_participant(&self, transfer_id: &str, window_label: &str) -> Result<(), String> {
        if self
            .active_transfers
            .lock()
            .get(transfer_id)
            .is_some_and(|transfer| {
                transfer.source_window == window_label || transfer.target_window == window_label
            })
        {
            Ok(())
        } else {
            Err("window is not part of an active transfer".to_string())
        }
    }

    fn abort_expired_transfer<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        transfer_id: &str,
    ) {
        let expired = self
            .active_transfers
            .lock()
            .get(transfer_id)
            .is_some_and(|transfer| transfer.created_at.elapsed() >= TRANSFER_TIMEOUT);
        if expired {
            self.abort_tab_transfer(app, transfer_id, "timeout");
        }
    }

    fn abort_expired_transfers<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>) {
        let expired: Vec<_> = self
            .active_transfers
            .lock()
            .values()
            .filter(|transfer| transfer.created_at.elapsed() >= TRANSFER_TIMEOUT)
            .map(|transfer| transfer.id.clone())
            .collect();
        for transfer_id in expired {
            self.abort_tab_transfer(app, &transfer_id, "timeout");
        }
    }

    fn emit_transfer_event<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        window_label: &str,
        event: &str,
        payload: serde_json::Value,
    ) {
        if let Err(error) = app.emit_to(window_label, event, payload) {
            tracing::error!("Failed to emit {event} to {window_label}: {error}");
        }
    }

    pub fn route_open_path(&self, app: &tauri::AppHandle, path_str: &str) {
        // Remote workspaces are URIs, not filesystem paths: nothing local to
        // canonicalize or stat.
        if path_str.starts_with("sworm://") {
            self.route_open_folder(app, path_str.to_string());
            return;
        }

        let requested = PathBuf::from(path_str);
        let canonical = match requested.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                tracing::warn!("Cannot open path {}: {error}", requested.display());
                return;
            }
        };
        let window = match resolve_effective_settings_for_folder_path(None) {
            Ok(resolved) => resolved.settings.window,
            Err(error) => {
                tracing::warn!("Cannot resolve open routing settings: {error}");
                WindowSettings::default()
            }
        };

        if canonical.is_dir() {
            self.route_open_folder(app, canonical.to_string_lossy().into_owned());
            return;
        }

        if !canonical.is_file() {
            tracing::warn!(
                "Open target is neither a file nor folder: {}",
                canonical.display()
            );
            return;
        }

        if let Some(label) = self.file_claim_owner(&canonical) {
            focus_window(app, &label);
            let folder = self
                .longest_claimed_folder(&canonical)
                .map(|(_, path)| path)
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new("/")).to_path_buf());
            self.queue_open_target(&label, file_target(&folder, &canonical), app);
            return;
        }

        let claimed = self.longest_claimed_folder(&canonical);
        let folder = claimed
            .as_ref()
            .map(|(_, path)| path.clone())
            .or_else(|| git_root(&canonical))
            .or_else(|| canonical.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("/"));
        let target = file_target(&folder, &canonical);
        match window.external_file_open_mode {
            ExternalFileOpenMode::PreferFolder => {
                if let Some((label, _)) = claimed {
                    self.queue_open_target(&label, target, app);
                } else {
                    self.open_window_with_target(app, target);
                }
            }
            ExternalFileOpenMode::FocusedWindow => {
                if let Some(label) = self.get_focused_window_label() {
                    self.queue_open_target(&label, target, app);
                } else {
                    self.open_window_with_target(app, target);
                }
            }
            ExternalFileOpenMode::NewWindow => self.open_window_with_target(app, target),
        }
    }

    fn route_open_folder(&self, app: &tauri::AppHandle, folder_path: String) {
        let mode = match resolve_effective_settings_for_folder_path(None) {
            Ok(resolved) => resolved.settings.window.external_folder_open_mode,
            Err(error) => {
                tracing::warn!("Cannot resolve open routing settings: {error}");
                WindowSettings::default().external_folder_open_mode
            }
        };
        let target = OpenTarget::Folder { folder_path };
        match mode {
            ExternalFolderOpenMode::FocusedWindow => match self.get_focused_window_label() {
                Some(label) => self.queue_open_target(&label, target, app),
                None => self.open_window_with_target(app, target),
            },
            ExternalFolderOpenMode::NewWindow => self.open_window_with_target(app, target),
        }
    }

    pub fn get_focused_window_label(&self) -> Option<String> {
        self.records
            .lock()
            .values()
            .max_by_key(|record| record.focus_order)
            .map(|record| record.label.clone())
    }

    pub fn remote_claimed(&self, server: &str) -> bool {
        let prefix = crate::router::Target::remote_uri(server, "");
        self.records.lock().values().any(|record| {
            record
                .folder_claims
                .iter()
                .chain(record.file_claims.keys())
                .any(|path| path.to_string_lossy().starts_with(&prefix))
        })
    }

    fn record_focus(&self, label: &str) {
        let mut records = self.records.lock();
        let next = records
            .values()
            .map(|record| record.focus_order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        if let Some(record) = records.get_mut(label) {
            record.focus_order = next;
        }
    }

    pub fn request_exit(&self) {
        self.exit_requested.store(true, Ordering::Release);
    }

    fn is_exit_requested(&self) -> bool {
        self.exit_requested.load(Ordering::Acquire)
    }

    /// A destroyed window discards its snapshot only on a sibling close:
    /// full exit and last-window close both leave the manifest intact for restore.
    pub(crate) fn destroy_discards_snapshot(&self) -> bool {
        !self.is_exit_requested() && self.records.lock().len() > 1
    }

    pub fn window_count(&self) -> usize {
        self.records.lock().len()
    }

    /// Quitting detaches wire-backed runs even while sibling windows are still
    /// being destroyed; only a genuine last-window close relies on the count.
    pub(crate) fn destroy_detaches_runs(&self) -> bool {
        self.is_exit_requested() || self.records.lock().is_empty()
    }

    pub(crate) fn has_window(&self, label: &str) -> bool {
        self.records.lock().contains_key(label)
    }

    pub(crate) fn owner_closed(&self, label: &str) -> bool {
        !self.has_window(label) && !self.destroy_detaches_runs()
    }

    fn next_focus_order(&self) -> usize {
        self.records
            .lock()
            .values()
            .map(|record| record.focus_order)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn create_window_with_target(
        &self,
        app: &tauri::AppHandle,
        target: OpenTarget,
    ) -> Result<String, String> {
        let window = self.create_workbench_window(app, None)?;
        self.queue_open_target(window.label(), target, app);
        Ok(window.label().to_owned())
    }

    fn open_window_with_target(&self, app: &tauri::AppHandle, target: OpenTarget) {
        if let Err(error) = self.create_window_with_target(app, target) {
            tracing::error!("Failed to create window for open target: {error}");
        }
    }

    fn file_claim_owner(&self, file: &Path) -> Option<String> {
        self.records
            .lock()
            .values()
            .filter(|record| record.file_claims.contains_key(file))
            .max_by_key(|record| record.focus_order)
            .map(|record| record.label.clone())
    }

    fn longest_claimed_folder(&self, file: &Path) -> Option<(String, PathBuf)> {
        self.records
            .lock()
            .values()
            .flat_map(|record| {
                record
                    .folder_claims
                    .iter()
                    .filter(move |folder| file.starts_with(folder))
                    .map(move |folder| (record.label.clone(), record.focus_order, folder.clone()))
            })
            .max_by(|left, right| {
                left.2
                    .components()
                    .count()
                    .cmp(&right.2.components().count())
                    .then(left.1.cmp(&right.1))
            })
            .map(|(label, _, folder)| (label, folder))
    }
    fn remove_window(&self, label: &str) -> Vec<PathBuf> {
        let mut records = self.records.lock();
        let Some(record) = records.remove(label) else {
            return Vec::new();
        };
        record
            .folder_claims
            .into_iter()
            .filter(|folder| {
                !records
                    .values()
                    .any(|record| record.folder_claims.contains(folder))
            })
            .collect()
    }
}
fn finish_window_close(
    app: &tauri::AppHandle,
    label: &str,
    discard_snapshot: bool,
    detach_runs: bool,
) {
    let state = app.state::<AppState>();
    state.windows.abort_transfers_for_window(app, label);
    tauri::async_runtime::block_on(state.windows.abort_groups_for_window(app, label));
    release_window_resources(&state, label, detach_runs);
    crate::deep_links::window_closed(app, label);
    if !discard_snapshot {
        return;
    }
    let db = state.host.db.write();
    let result = (|| -> Result<(), String> {
        let tx = db
            .conn()
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        AppStateKvService::delete(&tx, &format!("workbench:{label}"))?;
        state.windows.save_manifest_with(&tx)?;
        tx.commit().map_err(|error| error.to_string())
    })();
    if let Err(error) = result {
        tracing::error!("Failed to persist window close: {error}");
    }
}

fn release_window_resources(state: &AppState, label: &str, detach_runs: bool) {
    let final_folders = state.windows.remove_window(label);
    let protected = state.windows.protected_pty_runs(label);
    if detach_runs {
        state.host.detach_owner(label, &protected);
    } else {
        state.host.release_owner(label, &protected);
    }
    // Remote LSP servers have no local Host entry, so `release_owner` above
    // cannot see them. Both kills are owner-scoped and LSP session ids are
    // per window, so a close never reaches a surviving window's servers -
    // including a window that just adopted a transferred tab. PTY runs are
    // untouched: they may outlive the window.
    state.router.release_window(label);
    for folder in final_folders {
        release_folder_resources(state, &folder);
    }
}

/// Evict the shared resources of a folder that lost its last owning window.
pub(crate) fn release_folder_resources(state: &AppState, folder: &Path) {
    state.router.release_folder(&folder.to_string_lossy());
    state.host.release_folder(folder);
}

/// `folder` is always an ancestor of `file` here (claimed folder, git root, or parent dir).
fn file_target(folder: &Path, file: &Path) -> OpenTarget {
    let relative = file
        .strip_prefix(folder)
        .expect("open-target folder is an ancestor of the file");
    OpenTarget::File {
        folder_path: folder.to_string_lossy().into_owned(),
        file_path: relative.to_string_lossy().into_owned(),
    }
}

fn bounds_intersect_monitor(
    app: &tauri::AppHandle,
    bounds: &WindowBounds,
) -> Option<tauri::Monitor> {
    app.available_monitors()
        .ok()?
        .into_iter()
        .find(|monitor| bounds_intersect_rect(bounds, monitor.position(), monitor.size()))
}

fn bounds_intersect_rect(
    bounds: &WindowBounds,
    position: &PhysicalPosition<i32>,
    size: &PhysicalSize<u32>,
) -> bool {
    let left = i64::from(bounds.x);
    let top = i64::from(bounds.y);
    let right = left + i64::from(bounds.width);
    let bottom = top + i64::from(bounds.height);
    let monitor_left = i64::from(position.x);
    let monitor_top = i64::from(position.y);
    let monitor_right = monitor_left + i64::from(size.width);
    let monitor_bottom = monitor_top + i64::from(size.height);
    left < monitor_right && right > monitor_left && top < monitor_bottom && bottom > monitor_top
}

fn git_root(file: &Path) -> Option<PathBuf> {
    sworm_core::services::git::GitService::repo_root(file.parent()?)
}

pub(crate) fn focus_window(app: &tauri::AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        if let Err(error) = window.unminimize() {
            tracing::warn!(%label, %error, "Failed to unminimize window");
        }
        if let Err(error) = window.show() {
            tracing::warn!(%label, %error, "Failed to show window");
        }
        if let Err(error) = window.set_focus() {
            tracing::warn!(%label, %error, "Failed to focus window");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Receiver, TryRecvError};
    use tauri::Listener;

    #[test]
    fn only_drop_target_can_initiate_transfer() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "source", 1);
        add_window(&service, "target", 2);
        let app = tauri::test::mock_app();
        let params = TabTransferInitiateParams {
            source_window: "source".into(),
            target_window: "target".into(),
            tab_id: "tab".into(),
            target_index: 0,
        };
        for caller in ["source", "unrelated"] {
            assert!(service
                .initiate_tab_transfer(app.handle(), caller, params.clone())
                .is_err());
            assert!(service.active_transfers.lock().is_empty());
        }
        let id = service
            .initiate_tab_transfer(app.handle(), "target", params)
            .expect("drop target may initiate transfer");
        assert!(service
            .abort_tab_transfer_as(app.handle(), "unrelated", &id, "cancel")
            .is_err());
        assert!(service.active_transfers.lock().contains_key(&id));
        service
            .abort_tab_transfer_as(app.handle(), "source", &id, "cancel")
            .expect("source may abort transfer");
        assert!(!service.active_transfers.lock().contains_key(&id));
    }

    #[test]
    fn final_shutdown_waits_for_window_cleanup_without_locking_the_queue() {
        let service = Arc::new(WindowCoordinatorService::new());
        let finished = service.track_cleanup();
        let waiter = service.clone();
        let (exited, exit) = channel();
        let worker = std::thread::spawn(move || {
            waiter.wait_for_cleanup();
            exited.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut waiting = false;
        while Instant::now() < deadline {
            waiting = service
                .pending_cleanup
                .try_lock()
                .is_some_and(|queue| queue.is_empty());
            if waiting {
                break;
            }
            std::thread::yield_now();
        }
        let premature_exit = exit.try_recv();
        // The waiter must not hold the queue lock while another window closes.
        let second = waiting.then(|| service.track_cleanup());
        finished.send(()).unwrap();
        worker.join().unwrap();
        assert!(waiting);
        assert!(matches!(premature_exit, Err(TryRecvError::Empty)));
        exit.recv_timeout(Duration::from_secs(5)).unwrap();
        second.unwrap().send(()).unwrap();
        service.wait_for_cleanup();
    }

    fn connection() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        conn.execute_batch(
            "CREATE TABLE app_state (
                key TEXT PRIMARY KEY,
                value_json TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
        .expect("create app_state table");
        conn
    }

    fn add_window(service: &WindowCoordinatorService, label: &str, focus_order: usize) {
        service.records.lock().insert(
            label.to_string(),
            LiveWindowRecord {
                label: label.to_string(),
                bounds: None,
                maximized: false,
                focus_order,
                ready: false,
                pending_open_targets: Vec::new(),
                folder_claims: HashSet::new(),
                file_claims: HashMap::new(),
            },
        );
    }
    fn listen_for_transfer_events(
        window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    ) -> Receiver<&'static str> {
        let (sender, receiver) = channel();
        for event in [
            "tab-transfer-request",
            "tab-transfer-import",
            "tab-transfer-committed",
            "tab-transfer-finalized",
            "tab-transfer-aborted",
        ] {
            let sender = sender.clone();
            window.listen(event, move |_| {
                sender.send(event).expect("record transfer event")
            });
        }
        receiver
    }

    fn assert_next_event(receiver: &Receiver<&'static str>, expected: &str) {
        assert_eq!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .expect("receive transfer event"),
            expected
        );
    }

    fn assert_no_event(receiver: &Receiver<&'static str>) {
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn test_tab_transfer_events_are_scoped_to_recipient_windows() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-source", 1);
        add_window(&service, "workbench-target", 2);

        let app = tauri::test::mock_app();
        let source = tauri::WebviewWindowBuilder::new(&app, "workbench-source", Default::default())
            .build()
            .expect("create source window");
        let target = tauri::WebviewWindowBuilder::new(&app, "workbench-target", Default::default())
            .build()
            .expect("create target window");
        let source_events = listen_for_transfer_events(&source);
        let target_events = listen_for_transfer_events(&target);

        let transfer_id = service
            .initiate_tab_transfer(
                app.handle(),
                target.label(),
                TabTransferInitiateParams {
                    source_window: source.label().to_string(),
                    target_window: target.label().to_string(),
                    tab_id: "tab-1".to_string(),
                    target_index: 0,
                },
            )
            .expect("initiate transfer");
        assert_next_event(&source_events, "tab-transfer-request");
        assert_no_event(&target_events);

        service
            .source_export_ready(
                app.handle(),
                source.label(),
                TabTransferExportPayload {
                    transfer_id: transfer_id.clone(),
                    tab: serde_json::json!({ "id": "tab-1", "kind": "launcher" }),
                    terminal_state: None,
                    model_state: None,
                },
            )
            .expect("stage export");
        assert_next_event(&target_events, "tab-transfer-import");
        assert_no_event(&source_events);

        service
            .target_stage_ready(app.handle(), target.label(), &transfer_id)
            .expect("commit transfer");
        assert_next_event(&source_events, "tab-transfer-committed");
        assert_next_event(&target_events, "tab-transfer-finalized");
        assert_no_event(&source_events);
        assert_no_event(&target_events);

        let aborted_id = service
            .initiate_tab_transfer(
                app.handle(),
                target.label(),
                TabTransferInitiateParams {
                    source_window: source.label().to_string(),
                    target_window: target.label().to_string(),
                    tab_id: "tab-2".to_string(),
                    target_index: 0,
                },
            )
            .expect("initiate aborted transfer");
        assert_next_event(&source_events, "tab-transfer-request");
        assert_no_event(&target_events);

        service
            .abort_tab_transfer_as(app.handle(), target.label(), &aborted_id, "test")
            .expect("target may abort transfer");
        assert_next_event(&source_events, "tab-transfer-aborted");
        assert_next_event(&target_events, "tab-transfer-aborted");
        assert_no_event(&source_events);
        assert_no_event(&target_events);
    }

    #[test]
    fn test_destroyed_window_settles_transfers() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-source", 1);
        add_window(&service, "workbench-target", 2);

        let app = tauri::test::mock_app();
        let source = tauri::WebviewWindowBuilder::new(&app, "workbench-source", Default::default())
            .build()
            .expect("create source window");
        let target = tauri::WebviewWindowBuilder::new(&app, "workbench-target", Default::default())
            .build()
            .expect("create target window");
        let source_events = listen_for_transfer_events(&source);
        let target_events = listen_for_transfer_events(&target);

        let target_closed_id = service
            .initiate_tab_transfer(
                app.handle(),
                target.label(),
                TabTransferInitiateParams {
                    source_window: source.label().to_string(),
                    target_window: target.label().to_string(),
                    tab_id: "tab-target-closed".to_string(),
                    target_index: 0,
                },
            )
            .expect("initiate target-close transfer");
        assert_next_event(&source_events, "tab-transfer-request");
        service.abort_transfers_for_window(app.handle(), target.label());
        assert_next_event(&source_events, "tab-transfer-aborted");
        assert_next_event(&target_events, "tab-transfer-aborted");
        assert!(!service
            .active_transfers
            .lock()
            .contains_key(&target_closed_id));

        let source_closed_id = service
            .initiate_tab_transfer(
                app.handle(),
                target.label(),
                TabTransferInitiateParams {
                    source_window: source.label().to_string(),
                    target_window: target.label().to_string(),
                    tab_id: "tab-source-closed".to_string(),
                    target_index: 0,
                },
            )
            .expect("initiate source-close transfer");
        assert_next_event(&source_events, "tab-transfer-request");
        service.abort_transfers_for_window(app.handle(), source.label());
        assert_next_event(&source_events, "tab-transfer-aborted");
        assert_next_event(&target_events, "tab-transfer-aborted");
        assert!(!service
            .active_transfers
            .lock()
            .contains_key(&source_closed_id));

        let file = PathBuf::from("/repo/file.rs");
        assert!(matches!(
            service.claim_file_record(source.label(), &file, "tab-exported"),
            ClaimFileResult::Claimed
        ));
        let exported_id = service
            .initiate_tab_transfer(
                app.handle(),
                target.label(),
                TabTransferInitiateParams {
                    source_window: source.label().to_string(),
                    target_window: target.label().to_string(),
                    tab_id: "tab-exported".to_string(),
                    target_index: 0,
                },
            )
            .expect("initiate exported transfer");
        assert_next_event(&source_events, "tab-transfer-request");
        service
            .source_export_ready(
                app.handle(),
                source.label(),
                TabTransferExportPayload {
                    transfer_id: exported_id.clone(),
                    tab: serde_json::json!({ "id": "tab-exported", "kind": "text" }),
                    terminal_state: None,
                    model_state: None,
                },
            )
            .expect("export transfer");
        assert_next_event(&target_events, "tab-transfer-import");

        service.abort_transfers_for_window(app.handle(), source.label());
        assert!(service.active_transfers.lock().contains_key(&exported_id));
        assert_no_event(&source_events);
        assert_no_event(&target_events);

        service.remove_window(source.label());
        service
            .target_stage_ready(app.handle(), target.label(), &exported_id)
            .expect("commit transfer after source close");
        assert_next_event(&source_events, "tab-transfer-committed");
        assert_next_event(&target_events, "tab-transfer-finalized");
        let target_claim = service
            .records
            .lock()
            .get(target.label())
            .and_then(|record| record.file_claims.get(&file))
            .cloned();
        assert_eq!(target_claim.as_deref(), Some("tab-exported"));
        assert_no_event(&source_events);
        assert_no_event(&target_events);
    }

    fn transfer_subscriber() -> (
        PtySubscriber,
        Receiver<Vec<u8>>,
        Receiver<sworm_protocol::pty::PtyEvent>,
    ) {
        let (output, output_rx) = channel();
        let (events, event_rx) = channel();
        (
            PtySubscriber {
                output: Arc::new(move |bytes| {
                    output.send(bytes).map_err(|error| error.to_string())
                }),
                events: Arc::new(move |event| {
                    events.send(event).map_err(|error| error.to_string())
                }),
            },
            output_rx,
            event_rx,
        )
    }

    fn receive_output_until(receiver: &Receiver<Vec<u8>>, marker: &str) -> String {
        let mut text = String::new();
        while !text.contains(marker) {
            let bytes = receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("receive PTY output");
            text.push_str(&String::from_utf8_lossy(&bytes));
        }
        text
    }

    fn receive_exit(receiver: &Receiver<sworm_protocol::pty::PtyEvent>, run_id: &str) {
        loop {
            if let sworm_protocol::pty::PtyEvent::Exit { run_id: id, code } = receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("receive PTY exit")
            {
                assert_eq!(id, run_id);
                assert_eq!(code, Some(0));
                break;
            }
        }
    }

    #[test]
    fn native_transfer_of_local_pty_preserves_owner_and_delivery_cursors() {
        use sworm_core::services::pty::PtyRunState;
        use sworm_core::Host;

        let temporary = tempfile::tempdir().expect("test directory");
        let service = Arc::new(WindowCoordinatorService::new());
        for (index, label) in ["source", "target", "other"].iter().enumerate() {
            add_window(&service, label, index);
        }
        let app = tauri::test::mock_app();
        for label in ["source", "target", "other"] {
            tauri::WebviewWindowBuilder::new(&app, label, Default::default())
                .build()
                .expect("create transfer window");
        }
        let host = Arc::new(
            Host::new(temporary.path().join("sworm.db"), Arc::new(|_| Ok(())))
                .expect("create local host"),
        );
        let router = crate::router::WorkspaceRouter::new(Arc::clone(&host));
        app.manage(AppState {
            host: Arc::clone(&host),
            router,
            windows: Arc::clone(&service),
        });

        for (scenario, commit, exit_while_paused) in [
            ("abort", false, true),
            ("abort-detached", false, true),
            ("commit", true, false),
            ("abort-live", false, false),
        ] {
            let run_id = format!("run-{scenario}");
            let tab_id = format!("tab-{scenario}");
            let (source_subscriber, source_output_rx, source_events) = transfer_subscriber();
            let mut source_output = Some(source_output_rx);
            host.pty
                .spawn(
                    run_id.clone(),
                    "/bin/sh",
                    &["-c", "stty -echo; while IFS= read -r line; do [ \"$line\" = quit ] && exit 0; printf '<%s>\\n' \"$line\"; done"],
                    None,
                    None,
                    80,
                    24,
                    Some(source_subscriber),
                    Some("source".to_string()),
                    None,
                    None,
                )
                .expect("spawn physical PTY");
            let source_pid = match source_events
                .recv_timeout(Duration::from_secs(5))
                .expect("Started")
            {
                sworm_protocol::pty::PtyEvent::Started {
                    run_id: id,
                    pid: Some(pid),
                } => {
                    assert_eq!(id, run_id);
                    pid
                }
                other => panic!("expected Started with PID, got {other:?}"),
            };
            host.pty
                .write(&run_id, b"before\n")
                .expect("write before pause");
            let original = receive_output_until(source_output.as_ref().unwrap(), "<before>");
            assert_eq!(original.matches("<before>").count(), 1);

            let transfer_id = service
                .initiate_tab_transfer(
                    app.handle(),
                    "target",
                    TabTransferInitiateParams {
                        source_window: "source".to_string(),
                        target_window: "target".to_string(),
                        tab_id: tab_id.clone(),
                        target_index: 0,
                    },
                )
                .expect("start transfer");
            assert!(host.pty.pause_owned(&run_id, "other").is_err());
            host.pty
                .pause_owned(&run_id, "source")
                .expect("pause source");
            let payload = TabTransferExportPayload {
                transfer_id: transfer_id.clone(),
                tab: serde_json::json!({ "id": tab_id, "kind": "session" }),
                terminal_state: Some(serde_json::json!({ "runId": run_id })),
                model_state: None,
            };
            assert!(service
                .source_export_ready(app.handle(), "other", payload.clone())
                .is_err());
            service
                .source_export_ready(app.handle(), "source", payload)
                .expect("export paused session");
            let protected = service.protected_pty_runs("source");
            assert!(protected.contains(&run_id));
            host.release_owner("source", &protected);
            assert_eq!(host.pty.run_state(&run_id), Some(PtyRunState::Live));
            assert!(service
                .target_stage_ready(app.handle(), "other", &transfer_id)
                .is_err());
            assert!(service
                .abort_tab_transfer_as(app.handle(), "other", &transfer_id, "cancel")
                .is_err());

            let (target_subscriber, target_output, target_events) = transfer_subscriber();
            assert!(service
                .attach_transfer_pty(
                    "wrong-id",
                    "target",
                    &run_id,
                    target_subscriber.clone(),
                    &host.pty
                )
                .is_err());
            assert!(service
                .attach_transfer_pty(
                    &transfer_id,
                    "other",
                    &run_id,
                    target_subscriber.clone(),
                    &host.pty
                )
                .is_err());
            assert!(service
                .attach_transfer_pty(
                    &transfer_id,
                    "target",
                    "other-run",
                    target_subscriber.clone(),
                    &host.pty
                )
                .is_err());
            assert!(host
                .pty
                .attach_same_owner(&run_id, Some("source"), target_subscriber.clone(), 80, 24)
                .is_err());

            host.pty
                .write(&run_id, b"during\n")
                .expect("write during pause");
            if exit_while_paused {
                host.pty
                    .write(&run_id, b"quit\n")
                    .expect("exit during pause");
                let deadline = Instant::now() + Duration::from_secs(5);
                while host.pty.run_state(&run_id) != Some(PtyRunState::Completed(Some(0))) {
                    assert!(Instant::now() < deadline, "PTY must complete while paused");
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            service
                .attach_transfer_pty(
                    &transfer_id,
                    "target",
                    &run_id,
                    target_subscriber,
                    &host.pty,
                )
                .expect("target attaches transferred PTY");
            let replayed = receive_output_until(&target_output, "<during>");
            assert_eq!(replayed.matches("<during>").count(), 1);
            assert!(!replayed.contains("<before>"));
            if !exit_while_paused {
                host.pty
                    .write(&run_id, b"live\n")
                    .expect("write while target attached");
                let live = receive_output_until(&target_output, "<live>");
                assert_eq!(live.matches("<live>").count(), 1);
            } else {
                receive_exit(&target_events, &run_id);
            }

            if scenario == "abort-detached" {
                drop(source_output.take());
                service.abort_tab_transfer(app.handle(), &transfer_id, "source channel closed");
                host.pty
                    .ensure_owner(&run_id, Some("source"))
                    .expect("source keeps completed PTY");
                let (new_view, new_output, new_events) = transfer_subscriber();
                host.pty
                    .attach_same_owner(&run_id, Some("source"), new_view, 80, 24)
                    .expect("restored source replays after detached abort");
                assert!(matches!(
                    new_events.recv_timeout(Duration::from_secs(5)).expect("replayed Started"),
                    sworm_protocol::pty::PtyEvent::Started { run_id: id, pid: Some(pid) }
                        if id == run_id && pid == source_pid
                ));
                let replayed = receive_output_until(&new_output, "<during>");
                assert_eq!(replayed.matches("<before>").count(), 1);
                assert_eq!(replayed.matches("<during>").count(), 1);
                receive_exit(&new_events, &run_id);
            } else if commit {
                host.pty
                    .write(&run_id, b"quit\n")
                    .expect("exit while target attached");
                receive_exit(&target_events, &run_id);
                service
                    .target_stage_ready(app.handle(), "target", &transfer_id)
                    .expect("commit completed PTY");
                host.pty
                    .ensure_owner(&run_id, Some("target"))
                    .expect("target owns completed PTY");
                assert!(host.pty.ensure_owner(&run_id, Some("source")).is_err());
                assert!(
                    source_output.as_ref().unwrap().try_recv().is_err(),
                    "source receives no committed target output"
                );
            } else {
                service.abort_tab_transfer(app.handle(), &transfer_id, "test abort");
                host.pty
                    .ensure_owner(&run_id, Some("source"))
                    .expect("source owns aborted PTY");
                let restored = receive_output_until(
                    source_output.as_ref().unwrap(),
                    if exit_while_paused {
                        "<during>"
                    } else {
                        "<live>"
                    },
                );
                assert_eq!(restored.matches("<during>").count(), 1);
                if !exit_while_paused {
                    assert_eq!(restored.matches("<live>").count(), 1);
                    host.pty
                        .write(&run_id, b"quit\n")
                        .expect("exit after abort");
                }
                receive_exit(&source_events, &run_id);
            }
            assert!(service
                .attach_transfer_pty(
                    &transfer_id,
                    "target",
                    &run_id,
                    transfer_subscriber().0,
                    &host.pty
                )
                .is_err());
            assert_eq!(
                host.pty.run_state(&run_id),
                Some(PtyRunState::Completed(Some(0)))
            );
            host.pty.kill(&run_id).expect("release completed PTY");
        }
    }

    #[test]
    fn test_legacy_manifest_migration() {
        let conn = connection();
        AppStateKvService::put(&conn, "workbench", r#"{"tabs":[]}"#)
            .expect("store legacy workbench");

        let manifest =
            WindowCoordinatorService::load_manifest(&conn).expect("migrate legacy workbench");

        assert_eq!(manifest.windows.len(), 1);
        assert!(AppStateKvService::get(&conn, "workbench")
            .expect("read legacy key")
            .is_none());
        let snapshot_key = format!("workbench:{}", manifest.windows[0].label);
        assert_eq!(
            AppStateKvService::get(&conn, &snapshot_key).expect("read migrated snapshot"),
            Some(r#"{"tabs":[]}"#.to_string())
        );
        let stored: WindowManifest = serde_json::from_str(
            &AppStateKvService::get(&conn, MANIFEST_KEY)
                .expect("read manifest")
                .expect("manifest stored"),
        )
        .expect("parse manifest");
        assert_eq!(stored.windows.len(), 1);
        assert_eq!(stored.windows[0].label, manifest.windows[0].label);
    }

    #[test]
    fn test_last_window_close_keeps_snapshot() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-a", 1);
        assert!(!service.destroy_discards_snapshot());

        add_window(&service, "workbench-b", 2);
        assert!(service.destroy_discards_snapshot());

        service.request_exit();
        assert!(!service.destroy_discards_snapshot());
    }

    #[test]
    fn test_quit_detaches_runs_before_the_final_window_is_destroyed() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-a", 1);
        add_window(&service, "workbench-b", 2);
        assert!(!service.destroy_detaches_runs());

        service.request_exit();
        service.remove_window("workbench-a");
        assert!(service.destroy_detaches_runs());
        assert!(!service.owner_closed("workbench-a"));
    }

    #[test]
    fn test_sibling_close_kills_runs_until_the_last_window_leaves() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-a", 1);
        add_window(&service, "workbench-b", 2);
        assert!(!service.owner_closed("workbench-a"));
        service.remove_window("workbench-a");
        assert!(!service.destroy_detaches_runs());
        assert!(service.owner_closed("workbench-a"));

        service.remove_window("workbench-b");
        assert!(service.destroy_detaches_runs());
        assert!(!service.owner_closed("workbench-b"));
    }

    #[test]
    fn test_bounds_intersection_monitor_fallback() {
        let position = PhysicalPosition::new(0, 0);
        let size = PhysicalSize::new(1920, 1080);
        let visible = WindowBounds {
            x: 100,
            y: 100,
            width: 800,
            height: 600,
        };
        let offscreen = WindowBounds {
            x: 2500,
            y: 100,
            width: 800,
            height: 600,
        };

        assert!(bounds_intersect_rect(&visible, &position, &size));
        assert!(!bounds_intersect_rect(&offscreen, &position, &size));
    }

    #[test]
    fn test_focus_order_mru() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-a", 1);
        add_window(&service, "workbench-b", 2);

        service.record_focus("workbench-a");
        assert_eq!(
            service.get_focused_window_label().as_deref(),
            Some("workbench-a")
        );
        service.record_focus("workbench-b");
        assert_eq!(
            service.get_focused_window_label().as_deref(),
            Some("workbench-b")
        );
    }

    #[test]
    fn test_file_claim_and_atomic_transfer() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-a", 1);
        add_window(&service, "workbench-b", 2);
        let file = Path::new("/repo/src/main.rs");

        assert!(matches!(
            service.claim_file_record("workbench-a", file, "tab-a"),
            ClaimFileResult::Claimed
        ));
        assert!(matches!(
            service.claim_file_record("workbench-b", file, "tab-b"),
            ClaimFileResult::Redirect {
                ref owner_label,
                ref tab_id
            } if owner_label == "workbench-a" && tab_id == "tab-a"
        ));

        service
            .transfer_file_claim("workbench-a", "workbench-b", file, "tab-a")
            .expect("transfer claim");
        let records = service.records.lock();
        assert!(!records["workbench-a"].file_claims.contains_key(file));
        assert_eq!(
            records["workbench-b"]
                .file_claims
                .get(file)
                .map(String::as_str),
            Some("tab-a")
        );
    }

    #[test]
    fn test_longest_ancestor_and_exact_file_routing() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-root", 3);
        add_window(&service, "workbench-src", 1);
        let file = Path::new("/repo/src/lib.rs");
        service.claim_folder("workbench-root", PathBuf::from("/repo"));
        service.claim_folder("workbench-src", PathBuf::from("/repo/src"));

        let (owner, folder) = service
            .longest_claimed_folder(file)
            .expect("folder claim matches");
        assert_eq!(owner, "workbench-src");
        assert_eq!(folder, Path::new("/repo/src"));

        assert!(matches!(
            service.claim_file_record("workbench-root", file, "tab-file"),
            ClaimFileResult::Claimed
        ));
        assert_eq!(
            service.file_claim_owner(file).as_deref(),
            Some("workbench-root")
        );

        match file_target(&folder, file) {
            OpenTarget::File {
                folder_path,
                file_path,
            } => {
                assert_eq!(folder_path, "/repo/src");
                assert_eq!(file_path, "lib.rs");
            }
            other => panic!("expected file target, got {other:?}"),
        }
    }

    #[test]
    fn test_folder_claim_final_owner_teardown() {
        let service = WindowCoordinatorService::new();
        add_window(&service, "workbench-a", 1);
        add_window(&service, "workbench-b", 2);
        let folder = Path::new("/repo");
        service.claim_folder("workbench-a", folder.to_path_buf());
        service.claim_folder("workbench-b", folder.to_path_buf());

        assert!(!service.release_folder("workbench-a", folder));
        assert!(service.release_folder("workbench-b", folder));
    }
}
