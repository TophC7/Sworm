use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::env::EnvProbeResult;
use serde::Serialize;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

/// Trivial health check to prove the IPC bridge works.
#[tauri::command]
pub fn health_ping() -> String {
    "pong".to_string()
}

/// Return basic app metadata.
#[tauri::command]
pub fn app_get_info() -> AppInfo {
    AppInfo {
        name: "ADE".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Database smoke test: open DB, run migrations, verify a query works.
#[tauri::command]
pub fn db_smoke_test(state: tauri::State<'_, AppState>) -> Result<String, ApiError> {
    let db = state.db.lock();
    db.smoke_test().map_err(|e| ApiError::Database(e.to_string()))
}

/// Keyring smoke test: write/read/delete a test secret.
#[tauri::command]
pub fn keyring_smoke_test(state: tauri::State<'_, AppState>) -> Result<String, ApiError> {
    state
        .credentials
        .smoke_test()
        .map_err(|e| ApiError::Keyring(e))
}

/// Return environment probe diagnostics.
#[tauri::command]
pub fn env_probe(state: tauri::State<'_, AppState>) -> EnvProbeResult {
    state.env.probe_result()
}
