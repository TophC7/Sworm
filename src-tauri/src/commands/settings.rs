use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::{
    provider::ProviderStatus,
    settings::{
        EffectiveSettings, FormattingSettings, GeneralSettings, ProviderConfigRecord,
        SettingsChangedEvent, SettingsDiagnostic, SettingsLayerKind,
    },
};
use crate::services::{
    settings::SettingsService,
    settings_patch::patch_top_level_section,
    settings_resolution::{
        provider_binary_overrides, provider_config_record, resolve_effective_settings,
        resolve_effective_settings_for_project_path, SettingsLayerLoad,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

pub const SETTINGS_CHANGED_EVENT: &str = "settings-changed";

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

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveSettingsPayload {
    pub settings: EffectiveSettings,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsLayerPayload {
    pub path: String,
    pub loaded: bool,
    pub value: Value,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsFileResult {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SaveProviderConfigInput {
    pub provider_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PatchSettingsSectionInput {
    pub section: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectSettingsFileInput {
    pub project_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EffectiveSettingsInput {
    pub project_path: Option<String>,
}

#[tauri::command]
pub async fn settings_get(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsPayload, ApiError> {
    watch_settings_paths(&app, &state, None);
    let resolved = resolve_effective_settings_for_project_path(None).map_err(ApiError::Internal)?;
    let overrides = provider_binary_overrides(&resolved.settings);

    let mut providers = state.providers.lock();
    let statuses = providers.detect_all(
        &state.env.merged_path,
        &overrides,
        Some(&state.env.detected_shell),
    );
    let entries = statuses
        .into_iter()
        .map(|provider| {
            let config = provider_config_record(&resolved.settings, &provider.id.to_string());
            ProviderSettingsEntry { provider, config }
        })
        .collect();

    Ok(SettingsPayload {
        general: resolved.settings.general,
        formatting: resolved.settings.formatting,
        providers: entries,
    })
}

#[tauri::command]
pub async fn settings_get_effective(
    input: EffectiveSettingsInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<EffectiveSettingsPayload, ApiError> {
    let project_path = input.project_path.map(PathBuf::from);
    watch_settings_paths(&app, &state, project_path.as_deref());
    let resolved = resolve_effective_settings_for_project_path(project_path.as_deref())
        .map_err(ApiError::Internal)?;
    Ok(EffectiveSettingsPayload {
        settings: resolved.settings,
        diagnostics: resolved.diagnostics,
    })
}

#[tauri::command]
pub async fn settings_get_global_layer(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsLayerPayload, ApiError> {
    watch_settings_paths(&app, &state, None);
    global_layer_payload()
}

#[tauri::command]
pub async fn settings_patch_global_section(
    input: PatchSettingsSectionInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SettingsLayerPayload, ApiError> {
    patch_global_section(&input.section, input.value)?;
    let payload = global_layer_payload()?;
    emit_settings_changed(
        &app,
        &state,
        SettingsLayerKind::Global,
        None,
        payload.diagnostics.clone(),
    )?;
    Ok(payload)
}

#[tauri::command]
pub async fn settings_create_global_file() -> Result<SettingsFileResult, ApiError> {
    let path = ensure_global_settings_file()?;
    Ok(SettingsFileResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn settings_open_global_file(
    app: tauri::AppHandle,
) -> Result<SettingsFileResult, ApiError> {
    let path = ensure_global_settings_file()?;
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|error| {
            ApiError::Internal(format!(
                "Failed to open global settings file {}: {}",
                path.display(),
                error
            ))
        })?;
    Ok(SettingsFileResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn settings_create_project_file(
    input: ProjectSettingsFileInput,
) -> Result<SettingsFileResult, ApiError> {
    let path = ensure_project_settings_file(PathBuf::from(input.project_path))?;
    Ok(SettingsFileResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn settings_open_project_file(
    input: ProjectSettingsFileInput,
) -> Result<SettingsFileResult, ApiError> {
    // Project files are opened by the frontend editor once the backend has
    // created the file and returned the absolute path.
    let path = ensure_project_settings_file(PathBuf::from(input.project_path))?;
    Ok(SettingsFileResult {
        path: path.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub async fn settings_set_general(
    settings: GeneralSettings,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<GeneralSettings, ApiError> {
    patch_global_section(
        "general",
        serde_json::to_value(&settings).map_err(|error| ApiError::Internal(error.to_string()))?,
    )?;
    let diagnostics = global_layer_payload()?.diagnostics;
    emit_settings_changed(&app, &state, SettingsLayerKind::Global, None, diagnostics)?;
    Ok(settings)
}

#[tauri::command]
pub async fn settings_set_formatting(
    formatting: FormattingSettings,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<FormattingSettings, ApiError> {
    patch_global_section(
        "formatting",
        serde_json::to_value(&formatting).map_err(|error| ApiError::Internal(error.to_string()))?,
    )?;
    let diagnostics = global_layer_payload()?.diagnostics;
    emit_settings_changed(&app, &state, SettingsLayerKind::Global, None, diagnostics)?;
    Ok(formatting)
}

#[tauri::command]
pub async fn settings_set_provider_config(
    config: SaveProviderConfigInput,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ProviderConfigRecord, ApiError> {
    let record = ProviderConfigRecord {
        provider_id: config.provider_id,
        enabled: config.enabled,
        binary_path_override: config.binary_path_override,
        extra_args: config.extra_args,
    };

    patch_global_provider(&record)?;
    let diagnostics = global_layer_payload()?.diagnostics;
    emit_settings_changed(&app, &state, SettingsLayerKind::Global, None, diagnostics)?;
    Ok(record)
}

fn watch_settings_paths(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
    project_path: Option<&Path>,
) {
    let generation = Arc::clone(&state.settings_generation);
    if let Err(error) = state.settings_watchers.watch_global(app, generation) {
        tracing::warn!("settings global watcher failed: {error}");
    }

    if let Some(project_path) = project_path {
        let generation = Arc::clone(&state.settings_generation);
        if let Err(error) = state
            .settings_watchers
            .watch_project(app, project_path, generation)
        {
            tracing::warn!("settings project watcher failed: {error}");
        }
    }
}

fn global_layer_payload() -> Result<SettingsLayerPayload, ApiError> {
    let path = SettingsService::global_settings_path().map_err(ApiError::Internal)?;
    let layer = load_settings_layer(SettingsLayerKind::Global, path);
    Ok(layer_payload(layer))
}

fn load_settings_layer(layer: SettingsLayerKind, path: PathBuf) -> SettingsLayerLoad {
    match SettingsService::read_jsonc_layer_or_empty(&path) {
        Ok(layer) => SettingsLayerLoad::Loaded(layer),
        Err(message) => SettingsLayerLoad::Invalid {
            layer,
            path,
            message,
        },
    }
}

fn layer_payload(layer: SettingsLayerLoad) -> SettingsLayerPayload {
    match layer {
        SettingsLayerLoad::Loaded(layer) => SettingsLayerPayload {
            path: layer.path.to_string_lossy().into_owned(),
            loaded: layer.loaded,
            value: layer.value,
            diagnostics: Vec::new(),
        },
        SettingsLayerLoad::Invalid {
            layer,
            path,
            message,
        } => {
            let resolved = resolve_effective_settings(
                SettingsLayerLoad::Invalid {
                    layer,
                    path: path.clone(),
                    message,
                },
                None,
                Vec::<String>::new(),
            );
            SettingsLayerPayload {
                path: path.to_string_lossy().into_owned(),
                loaded: false,
                value: json!({}),
                diagnostics: resolved.diagnostics,
            }
        }
    }
}

fn patch_global_section(section: &str, value: Value) -> Result<(), ApiError> {
    let path = ensure_global_settings_file()?;
    patch_section_file(&path, section, value)
}

fn patch_section_file(path: &PathBuf, section: &str, value: Value) -> Result<(), ApiError> {
    validate_top_level_section(section)?;
    let original = std::fs::read_to_string(path).map_err(|error| {
        ApiError::Io(format!(
            "Failed to read settings file {}: {}",
            path.display(),
            error
        ))
    })?;
    let patched =
        patch_top_level_section(&original, section, &value).map_err(ApiError::Internal)?;
    std::fs::write(path, patched).map_err(|error| {
        ApiError::Io(format!(
            "Failed to write settings file {}: {}",
            path.display(),
            error
        ))
    })
}

fn patch_global_provider(record: &ProviderConfigRecord) -> Result<(), ApiError> {
    let path = ensure_global_settings_file()?;
    patch_provider_file(&path, record)
}

fn patch_provider_file(path: &PathBuf, record: &ProviderConfigRecord) -> Result<(), ApiError> {
    let original = std::fs::read_to_string(path).map_err(|error| {
        ApiError::Io(format!(
            "Failed to read settings file {}: {}",
            path.display(),
            error
        ))
    })?;
    let mut root = if original.trim().is_empty() {
        json!({})
    } else {
        jsonc_parser::parse_to_serde_value::<Value>(&original, &Default::default())
            .map_err(|error| ApiError::Internal(format!("Invalid settings JSONC: {error}")))?
    };
    let providers = ensure_object_property(&mut root, "providers")?;
    providers.insert(
        record.provider_id.clone(),
        json!({
            "enabled": record.enabled,
            "binary_path_override": record.binary_path_override,
            "extra_args": record.extra_args,
        }),
    );

    let patched =
        patch_top_level_section(&original, "providers", &Value::Object(providers.clone()))
            .map_err(ApiError::Internal)?;
    std::fs::write(path, patched).map_err(|error| {
        ApiError::Io(format!(
            "Failed to write settings file {}: {}",
            path.display(),
            error
        ))
    })
}

fn ensure_object_property<'a>(
    root: &'a mut Value,
    key: &str,
) -> Result<&'a mut Map<String, Value>, ApiError> {
    if root.is_null() {
        *root = json!({});
    }
    let object = root
        .as_object_mut()
        .ok_or_else(|| ApiError::InvalidArgument("Settings root must be an object".to_string()))?;
    let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
    if value.is_null() {
        *value = json!({});
    }
    value.as_object_mut().ok_or_else(|| {
        ApiError::InvalidArgument(format!("Settings `{key}` section must be an object"))
    })
}

fn validate_top_level_section(section: &str) -> Result<(), ApiError> {
    if matches!(section, "general" | "formatting" | "providers" | "lsp") {
        Ok(())
    } else {
        Err(ApiError::InvalidArgument(format!(
            "Unknown settings section: {section}"
        )))
    }
}

fn ensure_global_settings_file() -> Result<PathBuf, ApiError> {
    let path = SettingsService::ensure_global_settings_parent().map_err(ApiError::Internal)?;
    ensure_file_exists(&path)?;
    Ok(path)
}

fn ensure_project_settings_file(project_path: PathBuf) -> Result<PathBuf, ApiError> {
    let path =
        SettingsService::ensure_project_settings_parent(&project_path).map_err(ApiError::Io)?;
    ensure_file_exists(&path)?;
    Ok(path)
}

fn ensure_file_exists(path: &PathBuf) -> Result<(), ApiError> {
    if path.exists() {
        return Ok(());
    }
    std::fs::write(path, "{\n}\n").map_err(|error| {
        ApiError::Io(format!(
            "Failed to create settings file {}: {}",
            path.display(),
            error
        ))
    })
}

fn emit_settings_changed(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, AppState>,
    layer: SettingsLayerKind,
    project_id: Option<String>,
    diagnostics: Vec<SettingsDiagnostic>,
) -> Result<(), ApiError> {
    let generation = {
        let mut generation = state.settings_generation.lock();
        *generation += 1;
        *generation
    };
    app.emit(
        SETTINGS_CHANGED_EVENT,
        SettingsChangedEvent {
            layer,
            project_id,
            generation,
            diagnostics,
        },
    )
    .map_err(|error| ApiError::Internal(format!("Failed to emit settings-changed: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::settings::ProviderSettings;
    use std::fs;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sworm-settings-commands-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn validates_known_top_level_sections() {
        for section in ["general", "formatting", "providers", "lsp"] {
            validate_top_level_section(section).expect("section valid");
        }
        assert!(validate_top_level_section("nope").is_err());
    }

    #[test]
    fn provider_record_maps_effective_provider_config() {
        let mut settings = EffectiveSettings::default();
        settings.providers.insert(
            "claude_code".to_string(),
            ProviderSettings {
                enabled: false,
                binary_path_override: Some("/bin/claude".to_string()),
                extra_args: vec!["--debug".to_string()],
            },
        );

        let record = provider_config_record(&settings, "claude_code");

        assert_eq!(record.provider_id, "claude_code");
        assert!(!record.enabled);
        assert_eq!(record.binary_path_override.as_deref(), Some("/bin/claude"));
        assert_eq!(record.extra_args, vec!["--debug"]);
    }

    #[test]
    fn binary_overrides_skip_empty_paths() {
        let mut settings = EffectiveSettings::default();
        settings.providers.insert(
            "claude_code".to_string(),
            ProviderSettings {
                enabled: true,
                binary_path_override: Some("/bin/claude".to_string()),
                extra_args: Vec::new(),
            },
        );
        settings.providers.insert(
            "codex".to_string(),
            ProviderSettings {
                enabled: true,
                binary_path_override: Some(" ".to_string()),
                extra_args: Vec::new(),
            },
        );

        let overrides = provider_binary_overrides(&settings);

        assert_eq!(
            overrides.get("claude_code"),
            Some(&"/bin/claude".to_string())
        );
        assert!(!overrides.contains_key("codex"));
    }

    #[test]
    fn ensure_project_settings_file_creates_parent_and_file() {
        let root = temp_root("project-file");
        fs::create_dir_all(&root).expect("project root created");

        let path = ensure_project_settings_file(root.clone()).expect("settings file created");

        assert_eq!(path, root.join(".sworm/settings.jsonc"));
        assert_eq!(fs::read_to_string(&path).expect("file read"), "{\n}\n");
        fs::remove_dir_all(root).expect("temp dir removed");
    }

    #[test]
    fn patch_section_file_updates_jsonc_and_preserves_unknown_top_level() {
        let root = temp_root("patch-section");
        fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        fs::write(
            &path,
            r#"{
  "future_top": true,
  "general": { "terminal_font_size": 13 }
}
"#,
        )
        .expect("settings file written");

        patch_section_file(&path, "general", json!({ "terminal_font_size": 16 }))
            .expect("section patched");

        let patched = fs::read_to_string(&path).expect("patched file read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(parsed["general"]["terminal_font_size"], json!(16));
        fs::remove_dir_all(root).expect("temp dir removed");
    }

    #[test]
    fn patch_provider_file_merges_one_provider_into_providers_section() {
        let root = temp_root("patch-provider");
        fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        fs::write(
            &path,
            r#"{
  "providers": {
    "codex": { "enabled": false }
  }
}
"#,
        )
        .expect("settings file written");

        patch_provider_file(
            &path,
            &ProviderConfigRecord {
                provider_id: "claude_code".to_string(),
                enabled: true,
                binary_path_override: Some("/bin/claude".to_string()),
                extra_args: vec!["--debug".to_string()],
            },
        )
        .expect("provider patched");

        let patched = fs::read_to_string(&path).expect("patched file read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["providers"]["codex"]["enabled"], json!(false));
        assert_eq!(
            parsed["providers"]["claude_code"]["binary_path_override"],
            json!("/bin/claude")
        );
        fs::remove_dir_all(root).expect("temp dir removed");
    }
}
