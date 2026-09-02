use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::activity_map::DiscoveredProject;
use crate::services::activity_map::ActivityMapService;

/// Return the cached activity map, scanning on first call.
///
/// The scan runs outside the lock to avoid holding the mutex during
/// filesystem I/O.
#[tauri::command]
pub async fn activity_map_get(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    if let Some(cached) = &*state.activity_map_cache.lock() {
        return Ok(cached.clone());
    }

    let results = ActivityMapService::scan();

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
    let results = ActivityMapService::scan();

    let mut cache = state.activity_map_cache.lock();
    *cache = Some(results.clone());
    Ok(results)
}
