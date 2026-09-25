use crate::app_state::AppState;
use serde_json::Value;
use sworm_core::errors::ApiError;
use sworm_protocol::settings::{SettingsFileResult, ShortcutsFilePayload};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn shortcuts_get_global(
    state: tauri::State<'_, AppState>,
) -> Result<ShortcutsFilePayload, ApiError> {
    state.router.shortcuts_get_global().await
}

#[tauri::command]
pub async fn shortcuts_set_global(
    value: Value,
    state: tauri::State<'_, AppState>,
) -> Result<ShortcutsFilePayload, ApiError> {
    state.router.shortcuts_set_global(value).await
}

#[tauri::command]
pub async fn shortcuts_create_global_file(
    state: tauri::State<'_, AppState>,
) -> Result<SettingsFileResult, ApiError> {
    state.router.shortcuts_create_global_file().await
}

#[tauri::command]
pub async fn shortcuts_open_global_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsFileResult, ApiError> {
    let result = state.router.shortcuts_create_global_file().await?;
    app.opener()
        .open_path(result.path.clone(), None::<&str>)
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to open shortcuts file {}: {}",
                result.path, error
            ))
        })?;
    Ok(result)
}
