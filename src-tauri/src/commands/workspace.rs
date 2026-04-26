// Commands exposed to the frontend for workspace-layout persistence.

use crate::app_state::AppState;
use crate::errors::ApiError;

/// Read the persisted workspace blob for a project. Returns `None`
/// when the project has never been saved (first open).
#[tauri::command]
pub fn workspace_state_get(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    let db = state.db.read();
    state
        .workspace_state
        .get(db.conn(), &project_id)
        .map_err(ApiError::Database)
}

/// Persist the workspace blob for a project. The frontend owns the
/// shape of `state_json` — this is an opaque string on the backend.
#[tauri::command]
pub fn workspace_state_put(
    project_id: String,
    state_json: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.write();
    state
        .workspace_state
        .put(db.conn(), &project_id, &state_json)
        .map_err(ApiError::Database)
}

/// Read a value from the app-state key/value store. Returns `None`
/// when no entry exists for the key.
#[tauri::command]
pub fn app_state_get(
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    let db = state.db.read();
    state
        .app_state_kv
        .get(db.conn(), &key)
        .map_err(ApiError::Database)
}

/// Write a value to the app-state key/value store.
#[tauri::command]
pub fn app_state_put(
    key: String,
    value_json: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.write();
    state
        .app_state_kv
        .put(db.conn(), &key, &value_json)
        .map_err(ApiError::Database)
}
