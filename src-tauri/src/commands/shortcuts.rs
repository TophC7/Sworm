use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::settings::SettingsFileResult;
use tauri_plugin_opener::OpenerExt;

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
