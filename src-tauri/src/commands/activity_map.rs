use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::activity_map::DiscoveredProject;

#[tauri::command]
pub async fn activity_map_get(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    state.host.activity_map_get().await
}

#[tauri::command]
pub async fn activity_map_refresh(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DiscoveredProject>, ApiError> {
    state.host.activity_map_refresh().await
}
