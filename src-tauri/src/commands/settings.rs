use crate::app_state::AppState;
use crate::router::Target;
use sworm_core::errors::ApiError;
use sworm_protocol::settings::{
    EffectiveSettingsInput, EffectiveSettingsPayload, FolderSettingsFileInput, FormattingSettings,
    NixSettings, PatchSettingsSectionInput, ProviderConfigRecord, SaveProviderConfigInput,
    SettingsFileResult, SettingsLayerPayload, SettingsPayload, TerminalSettings, WindowSettings,
};
use tauri_plugin_opener::OpenerExt;

/// The host-section editor must read the exact layer it writes: a remote
/// workspace's global layer lives on its daemon.
#[tauri::command]
pub async fn settings_get(
    folder_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsPayload, ApiError> {
    state
        .router
        .settings_get(settings_server(folder_path.as_deref())?)
        .await
}

#[tauri::command]
pub async fn settings_get_effective(
    input: EffectiveSettingsInput,
    state: tauri::State<'_, AppState>,
) -> Result<EffectiveSettingsPayload, ApiError> {
    state.router.settings_get_effective(input).await
}

#[tauri::command]
pub async fn settings_get_global_layer(
    state: tauri::State<'_, AppState>,
) -> Result<SettingsLayerPayload, ApiError> {
    state.host.settings_get_global_layer().await
}

#[tauri::command]
pub async fn settings_patch_global_section(
    input: PatchSettingsSectionInput,
    folder_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsLayerPayload, ApiError> {
    let server = settings_server(folder_path.as_deref())?;
    state
        .router
        .settings_patch_global_section(server, input)
        .await
}

#[tauri::command]
pub async fn settings_create_global_file(
    state: tauri::State<'_, AppState>,
) -> Result<SettingsFileResult, ApiError> {
    state.host.settings_create_global_file().await
}

#[tauri::command]
pub async fn settings_open_global_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsFileResult, ApiError> {
    let result = state.host.settings_create_global_file().await?;
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

#[tauri::command]
pub async fn settings_open_folder_file(
    input: FolderSettingsFileInput,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsFileResult, ApiError> {
    state.router.settings_open_folder_file(input).await
}

#[tauri::command]
pub async fn settings_set_window(
    settings: WindowSettings,
    state: tauri::State<'_, AppState>,
) -> Result<WindowSettings, ApiError> {
    state.host.settings_set_window(settings).await
}

#[tauri::command]
pub async fn settings_set_terminal(
    settings: TerminalSettings,
    state: tauri::State<'_, AppState>,
) -> Result<TerminalSettings, ApiError> {
    state.host.settings_set_terminal(settings).await
}

#[tauri::command]
pub async fn settings_set_nix(
    settings: NixSettings,
    folder_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<NixSettings, ApiError> {
    let server = settings_server(folder_path.as_deref())?;
    state.router.settings_set_nix(server, settings).await
}

#[tauri::command]
pub async fn settings_set_formatting(
    formatting: FormattingSettings,
    folder_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<FormattingSettings, ApiError> {
    let server = settings_server(folder_path.as_deref())?;
    state
        .router
        .settings_set_formatting(server, formatting)
        .await
}

#[tauri::command]
pub async fn settings_set_provider_config(
    config: SaveProviderConfigInput,
    folder_path: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<ProviderConfigRecord, ApiError> {
    let server = settings_server(folder_path.as_deref())?;
    state
        .router
        .settings_set_provider_config(server, config)
        .await
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
