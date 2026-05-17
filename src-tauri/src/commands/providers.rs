use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::provider::ProviderStatus;
use crate::services::settings_resolution::{
    provider_binary_overrides, resolve_effective_settings_for_project_path,
};

/// List all providers with their detection status.
#[tauri::command]
pub async fn provider_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    let merged_path = state.env.merged_path.clone();
    let effective =
        resolve_effective_settings_for_project_path(None).map_err(ApiError::Internal)?;
    let overrides = provider_binary_overrides(&effective.settings);

    let mut providers = state.providers.lock();
    Ok(providers.list(&merged_path, &overrides, Some(&state.env.detected_shell)))
}

/// Force re-detect all providers.
#[tauri::command]
pub async fn provider_refresh(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    let merged_path = state.env.merged_path.clone();
    let effective =
        resolve_effective_settings_for_project_path(None).map_err(ApiError::Internal)?;
    let overrides = provider_binary_overrides(&effective.settings);

    let mut providers = state.providers.lock();
    Ok(providers.detect_all(&merged_path, &overrides, Some(&state.env.detected_shell)))
}
