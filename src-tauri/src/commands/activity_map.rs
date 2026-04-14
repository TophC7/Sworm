use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::activity_map::DiscoveredProject;
use crate::services::activity_map::ActivityMapService;

/// Read sworm projects under the db lock (consistent lock ordering).
fn load_sworm_projects(state: &AppState) -> Result<Vec<(String, String)>, ApiError> {
    let db = state.db.lock();
    state
        .projects
        .list(db.conn())
        .map_err(ApiError::Database)
        .map(|ps| ps.into_iter().map(|p| (p.path, p.id)).collect())
}

/// Return the cached activity map, scanning on first call.
///
/// Lock ordering: db first, then activity_map_cache, to match refresh
/// and prevent deadlock. The scan runs outside the lock to avoid
/// holding the mutex during filesystem I/O.
#[tauri::command]
pub fn activity_map_get(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    {
        let cache = state.activity_map_cache.lock();
        if let Some(ref cached) = *cache {
            return Ok(cached.clone());
        }
    }

    let sworm_projects = load_sworm_projects(&state)?;

    // Scan outside any lock
    let results = ActivityMapService::scan(&sworm_projects);

    let mut cache = state.activity_map_cache.lock();
    *cache = Some(results.clone());
    Ok(results)
}

/// Force rescan of all external agent history and return fresh results.
///
/// The scan runs outside the lock to avoid holding the mutex during
/// filesystem I/O.
#[tauri::command]
pub fn activity_map_refresh(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    let sworm_projects = load_sworm_projects(&state)?;

    // Scan outside the lock
    let results = ActivityMapService::scan(&sworm_projects);

    let mut cache = state.activity_map_cache.lock();
    *cache = Some(results.clone());
    Ok(results)
}
