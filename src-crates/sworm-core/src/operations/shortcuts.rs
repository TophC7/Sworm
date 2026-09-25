use crate::{errors::ApiError, services::settings::SettingsService, Host};
use serde_json::Value;
use std::path::PathBuf;
use sworm_protocol::settings::{SettingsFileResult, ShortcutsFilePayload};

const SHORTCUTS_FILE_TEMPLATE: &str =
    "{\n  \"version\": 1,\n  \"bindings\": [],\n  \"unboundCommands\": []\n}\n";

impl Host {
    pub async fn shortcuts_get_global(&self) -> Result<ShortcutsFilePayload, ApiError> {
        let path = SettingsService::global_shortcuts_path().map_err(ApiError::Internal)?;
        let layer =
            SettingsService::read_jsonc_layer_or_empty(&path).map_err(ApiError::Internal)?;
        Ok(ShortcutsFilePayload {
            path: layer.path.to_string_lossy().into_owned(),
            loaded: layer.loaded,
            value: layer.value,
        })
    }

    pub async fn shortcuts_set_global(
        &self,
        value: Value,
    ) -> Result<ShortcutsFilePayload, ApiError> {
        if !value.is_object() {
            return Err(ApiError::InvalidArgument(
                "Shortcuts file root must be an object".to_string(),
            ));
        }

        let path = ensure_global_shortcuts_file()?;
        let serialized = serde_json::to_string_pretty(&value).map_err(|error| {
            ApiError::Internal(format!("Failed to serialize shortcuts: {error}"))
        })?;
        std::fs::write(&path, format!("{serialized}\n")).map_err(|error| {
            ApiError::Io(format!(
                "Failed to write shortcuts file {}: {}",
                path.display(),
                error
            ))
        })?;

        Ok(ShortcutsFilePayload {
            path: path.to_string_lossy().into_owned(),
            loaded: true,
            value,
        })
    }

    pub async fn shortcuts_create_global_file(&self) -> Result<SettingsFileResult, ApiError> {
        let path = ensure_global_shortcuts_file()?;
        Ok(SettingsFileResult {
            path: path.to_string_lossy().into_owned(),
        })
    }
}

fn ensure_global_shortcuts_file() -> Result<PathBuf, ApiError> {
    let path = SettingsService::ensure_global_shortcuts_parent().map_err(ApiError::Internal)?;
    if !path.exists() {
        std::fs::write(&path, SHORTCUTS_FILE_TEMPLATE).map_err(|error| {
            ApiError::Io(format!(
                "Failed to create shortcuts file {}: {}",
                path.display(),
                error
            ))
        })?;
    }
    Ok(path)
}
