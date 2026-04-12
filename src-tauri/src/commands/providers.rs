use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::provider::ProviderStatus;
use crate::services::settings::SettingsService;

/// List all providers with their detection status.
#[tauri::command]
pub fn provider_list(state: tauri::State<'_, AppState>) -> Result<Vec<ProviderStatus>, ApiError> {
    let merged_path = state.env.merged_path.clone();
    let db = state.db.lock();
    let overrides =
        SettingsService::load_binary_overrides(db.conn()).map_err(ApiError::Database)?;

    let mut providers = state.providers.lock();
    Ok(providers.list(&merged_path, &overrides, Some(&state.env.detected_shell)))
}

/// Force re-detect all providers.
#[tauri::command]
pub fn provider_refresh(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    let merged_path = state.env.merged_path.clone();
    let db = state.db.lock();
    let overrides =
        SettingsService::load_binary_overrides(db.conn()).map_err(ApiError::Database)?;

    let mut providers = state.providers.lock();
    Ok(providers.detect_all(&merged_path, &overrides, Some(&state.env.detected_shell)))
}
