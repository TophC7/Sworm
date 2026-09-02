use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::provider::ProviderStatus;
use crate::services::settings_resolution::{
    provider_binary_overrides, resolve_effective_settings_for_folder_path,
};

/// List all providers with their detection status.
#[tauri::command]
pub async fn provider_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    let merged_path = state.env.merged_path.clone();
    let effective = resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
    let overrides = provider_binary_overrides(&effective.settings);

    Ok(state
        .providers
        .detect_all(&merged_path, &overrides, Some(&state.env.detected_shell)))
}
