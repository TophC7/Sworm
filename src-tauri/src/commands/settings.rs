use crate::app_state::AppState;
use crate::router::Target;
use sworm_core::errors::ApiError;
use sworm_protocol::settings::SettingsFileResult;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn settings_open_global_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsFileResult, ApiError> {
    let result = state.router.settings_create_global_file().await?;
    app.opener()
        .open_path(result.path.clone(), None::<&str>)
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to open global settings file {}: {}",
                result.path, error
            ))
        })?;
    Ok(result)
}

/// Host settings sections live on the machine that runs the folder. The
/// frontend passes the active folder so a remote workspace's edits land on its
/// daemon; an absent or local folder keeps them on the desktop.
pub(crate) fn settings_server(folder_path: Option<&str>) -> Result<Option<String>, ApiError> {
    match folder_path {
        Some(path) => match Target::parse(path)? {
            Target::Remote { server, .. } => Ok(Some(server.to_owned())),
            Target::Local => Ok(None),
        },
        None => Ok(None),
    }
}
