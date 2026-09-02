use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::activity_map::DiscoveredProject;
use crate::services::activity_map::ActivityMapService;

/// Key in `app_state` holding the frontend's MRU folder list (JSON `string[]`).
const RECENT_FOLDERS_KEY: &str = "recent_folders";

/// Read recent folders under the db lock (consistent lock ordering).
fn load_recent_folders(state: &AppState) -> Result<Vec<String>, ApiError> {
    let db = state.db.read();
    let raw = state
        .app_state_kv
        .get(db.conn(), RECENT_FOLDERS_KEY)
        .map_err(ApiError::Database)?;
    Ok(raw
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// Return the cached activity map, scanning on first call.
///
/// Lock ordering: db first, then activity_map_cache, to match refresh
/// and prevent deadlock. The scan runs outside the lock to avoid
/// holding the mutex during filesystem I/O.
#[tauri::command]
pub async fn activity_map_get(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    if let Some(cached) = &*state.activity_map_cache.lock() {
        return Ok(cached.clone());
    }

    let recent = load_recent_folders(&state)?;

    // Scan outside any lock
    let results = ActivityMapService::scan(&recent);

    let mut cache = state.activity_map_cache.lock();
    *cache = Some(results.clone());
    Ok(results)
}

/// Force rescan of all external agent history and return fresh results.
///
/// The scan runs outside the lock to avoid holding the mutex during
/// filesystem I/O.
#[tauri::command]
pub async fn activity_map_refresh(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    let recent = load_recent_folders(&state)?;

    // Scan outside the lock
    let results = ActivityMapService::scan(&recent);

    let mut cache = state.activity_map_cache.lock();
    *cache = Some(results.clone());
    Ok(results)
}
