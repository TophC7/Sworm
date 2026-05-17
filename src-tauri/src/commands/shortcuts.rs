use crate::errors::ApiError;
use crate::services::settings::SettingsService;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use tauri_plugin_opener::OpenerExt;

const SHORTCUTS_FILE_TEMPLATE: &str =
    "{\n  \"version\": 1,\n  \"bindings\": [],\n  \"unboundCommands\": []\n}\n";

#[derive(Debug, Clone, Serialize)]
pub struct ShortcutsFilePayload {
    pub path: String,
    pub loaded: bool,
    pub value: Value,
}

#[tauri::command]
pub async fn shortcuts_get_global() -> Result<ShortcutsFilePayload, ApiError> {
    let path = SettingsService::global_shortcuts_path().map_err(ApiError::Internal)?;
    let layer = SettingsService::read_jsonc_layer_or_empty(&path).map_err(ApiError::Internal)?;
    Ok(ShortcutsFilePayload {
        path: layer.path.to_string_lossy().into_owned(),
        loaded: layer.loaded,
        value: layer.value,
    })
}

#[tauri::command]
pub async fn shortcuts_set_global(value: Value) -> Result<ShortcutsFilePayload, ApiError> {
    if !value.is_object() {
        return Err(ApiError::InvalidArgument(
            "Shortcuts file root must be an object".to_string(),
        ));
    }

    let path = ensure_global_shortcuts_file()?;
    let serialized = serde_json::to_string_pretty(&value)
        .map_err(|error| ApiError::Internal(format!("Failed to serialize shortcuts: {error}")))?;
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

#[tauri::command]
pub async fn shortcuts_create_global_file() -> Result<ShortcutsFileResult, ApiError> {
    let path = ensure_global_shortcuts_file()?;
    Ok(ShortcutsFileResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn shortcuts_open_global_file(
    app: tauri::AppHandle,
) -> Result<ShortcutsFileResult, ApiError> {
    let path = ensure_global_shortcuts_file()?;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to open shortcuts file {}: {}",
                path.display(),
                error
            ))
        })?;
    Ok(ShortcutsFileResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ShortcutsFileResult {
    pub path: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("sworm-shortcuts-{name}-{}", Uuid::new_v4()))
            .join("shortcuts.jsonc")
    }

    #[test]
    fn template_parses_as_jsonc_object() {
        let value = jsonc_parser::parse_to_serde_value::<Value>(
            SHORTCUTS_FILE_TEMPLATE,
            &Default::default(),
        )
        .expect("template parses");
        assert_eq!(value["version"], json!(1));
        assert!(value["bindings"].as_array().is_some());
    }

    #[test]
    fn shortcuts_file_round_trip_shape_is_object() {
        let value = json!({
            "version": 1,
            "bindings": [{ "command": "toggle-command-palette", "key": "Ctrl+P" }],
            "unboundCommands": []
        });
        assert!(value.is_object());
        let rendered = serde_json::to_string_pretty(&value).expect("serializes");
        assert!(rendered.contains("toggle-command-palette"));
    }

    #[test]
    fn temp_file_helper_keeps_shortcuts_filename() {
        let path = temp_file("path");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("shortcuts.jsonc")
        );
    }
}
