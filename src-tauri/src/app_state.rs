use crate::host_events::host_event_sink;
use crate::router::WorkspaceRouter;
use crate::services::{app_state_kv::AppStateKvService, windows::WindowCoordinatorService};
use std::path::PathBuf;
use std::sync::Arc;
use sworm_core::Host;
use tauri::Manager;

/// Resolve the default database path inside the Tauri app data directory.
fn resolve_db_path(app_handle: &tauri::AppHandle) -> Result<PathBuf, anyhow::Error> {
    let app_data = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| anyhow::anyhow!("Failed to resolve app data dir: {}", e))?;
    Ok(app_data.join("sworm.db"))
}

/// Desktop ownership and IPC adapters around the local host.
pub struct AppState {
    pub host: Arc<Host>,
    pub router: WorkspaceRouter,
    pub windows: Arc<WindowCoordinatorService>,
    pub app_state_kv: AppStateKvService,
}

impl AppState {
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let windows = Arc::new(WindowCoordinatorService::new());
        let events = host_event_sink(app_handle.clone(), Arc::clone(&windows));
        let host = Arc::new(Host::new(resolve_db_path(app_handle)?, events)?);
        let router = WorkspaceRouter::new(Arc::clone(&host));
        Ok(Self {
            host,
            router,
            windows,
            app_state_kv: AppStateKvService::new(),
        })
    }
}
