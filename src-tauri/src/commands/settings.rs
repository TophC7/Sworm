use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::provider::ProviderStatus;
use crate::services::settings::{
    FormattingSettings, GeneralSettings, ProviderConfigRecord, SettingsService,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSettingsEntry {
    pub provider: ProviderStatus,
    pub config: ProviderConfigRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsPayload {
    pub general: GeneralSettings,
    pub formatting: FormattingSettings,
    pub providers: Vec<ProviderSettingsEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveProviderConfigInput {
    pub provider_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub extra_args: Vec<String>,
}

#[tauri::command]
pub fn settings_get(state: tauri::State<'_, AppState>) -> Result<SettingsPayload, ApiError> {
    let db = state.db.read();
    let general = SettingsService::get_general_settings(db.conn()).map_err(ApiError::Database)?;
    let formatting =
        SettingsService::get_formatting_settings(db.conn()).map_err(ApiError::Database)?;
    let overrides =
        SettingsService::load_binary_overrides(db.conn()).map_err(ApiError::Database)?;

    let mut providers = state.providers.lock();
    let statuses = providers.detect_all(
        &state.env.merged_path,
        &overrides,
        Some(&state.env.detected_shell),
    );
    let entries = statuses
        .into_iter()
        .map(|provider| {
            let config = SettingsService::get_provider_config(db.conn(), &provider.id.to_string())
                .map_err(ApiError::Database)?
                .unwrap_or_else(|| ProviderConfigRecord::default_for(&provider.id.to_string()));

            Ok(ProviderSettingsEntry { provider, config })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    Ok(SettingsPayload {
        general,
        formatting,
        providers: entries,
    })
}

#[tauri::command]
pub fn settings_set_general(
    settings: GeneralSettings,
    state: tauri::State<'_, AppState>,
) -> Result<GeneralSettings, ApiError> {
    let db = state.db.write();
    SettingsService::set_general_settings(db.conn(), &settings).map_err(ApiError::Database)?;
    Ok(settings)
}

#[tauri::command]
pub fn settings_set_formatting(
    formatting: FormattingSettings,
    state: tauri::State<'_, AppState>,
) -> Result<FormattingSettings, ApiError> {
    let db = state.db.write();
    SettingsService::set_formatting_settings(db.conn(), &formatting).map_err(ApiError::Database)?;
    Ok(formatting)
}

#[tauri::command]
pub fn settings_set_provider_config(
    config: SaveProviderConfigInput,
    state: tauri::State<'_, AppState>,
) -> Result<ProviderConfigRecord, ApiError> {
    let record = ProviderConfigRecord {
        provider_id: config.provider_id,
        enabled: config.enabled,
        binary_path_override: config.binary_path_override,
        extra_args: config.extra_args,
    };

    let db = state.db.write();
    SettingsService::save_provider_config(db.conn(), &record).map_err(ApiError::Database)?;
    Ok(record)
}
