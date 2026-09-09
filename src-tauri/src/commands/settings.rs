use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::settings::{
    EffectiveSettingsInput, EffectiveSettingsPayload, FolderSettingsFileInput, FormattingSettings,
    NixSettings, PatchSettingsSectionInput, ProviderConfigRecord, SaveProviderConfigInput,
    SettingsFileResult, SettingsLayerPayload, SettingsPayload, TerminalSettings, WindowSettings,
};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn settings_get(state: tauri::State<'_, AppState>) -> Result<SettingsPayload, ApiError> {
    state.host.settings_get().await
}

#[tauri::command]
pub async fn settings_get_effective(
    input: EffectiveSettingsInput,
    state: tauri::State<'_, AppState>,
) -> Result<EffectiveSettingsPayload, ApiError> {
    state.host.settings_get_effective(input).await
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
    state: tauri::State<'_, AppState>,
) -> Result<SettingsLayerPayload, ApiError> {
    state.host.settings_patch_global_section(input).await
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
    state.host.settings_open_folder_file(input).await
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
    state: tauri::State<'_, AppState>,
) -> Result<NixSettings, ApiError> {
    state.host.settings_set_nix(settings).await
}

#[tauri::command]
pub async fn settings_set_formatting(
    formatting: FormattingSettings,
    state: tauri::State<'_, AppState>,
) -> Result<FormattingSettings, ApiError> {
    state.host.settings_set_formatting(formatting).await
}

#[tauri::command]
pub async fn settings_set_provider_config(
    config: SaveProviderConfigInput,
    state: tauri::State<'_, AppState>,
) -> Result<ProviderConfigRecord, ApiError> {
    state.host.settings_set_provider_config(config).await
}
