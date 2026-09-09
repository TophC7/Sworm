use crate::{
    errors::ApiError,
    events::HostEvent,
    services::{
        settings::SettingsService,
        settings_patch::patch_top_level_section,
        settings_resolution::{
            parse_error_diagnostic, provider_binary_overrides, provider_config_record,
            resolve_effective_settings_for_folder_path, SettingsLayerLoad,
        },
    },
    Host,
};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use sworm_protocol::settings::{
    EffectiveSettingsInput, EffectiveSettingsPayload, FolderSettingsFileInput, FormattingSettings,
    NixSettings, PatchSettingsSectionInput, ProviderConfigRecord, ProviderSettingsEntry,
    SaveProviderConfigInput, SettingsChangedEvent, SettingsDiagnostic, SettingsFileResult,
    SettingsLayerKind, SettingsLayerPayload, SettingsPayload, TerminalSettings, WindowSettings,
};

impl Host {
    pub async fn settings_get(&self) -> Result<SettingsPayload, ApiError> {
        self.watch_settings_paths(None);
        let resolved =
            resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
        let overrides = provider_binary_overrides(&resolved.settings);
        let providers = self
            .providers
            .detect_all(
                &self.env.merged_path,
                &overrides,
                Some(&self.env.detected_shell),
            )
            .into_iter()
            .map(|provider| {
                let config = provider_config_record(&resolved.settings, &provider.id.to_string());
                ProviderSettingsEntry { provider, config }
            })
            .collect();

        Ok(SettingsPayload {
            window: resolved.settings.window,
            terminal: resolved.settings.terminal,
            nix: resolved.settings.nix,
            formatting: resolved.settings.formatting,
            providers,
        })
    }

    pub async fn settings_get_effective(
        &self,
        input: EffectiveSettingsInput,
    ) -> Result<EffectiveSettingsPayload, ApiError> {
        let folder_path = input.folder_path.map(PathBuf::from);
        self.watch_settings_paths(folder_path.as_deref());
        let resolved = resolve_effective_settings_for_folder_path(folder_path.as_deref())
            .map_err(ApiError::Internal)?;
        Ok(EffectiveSettingsPayload {
            settings: resolved.settings,
            diagnostics: resolved.diagnostics,
        })
    }

    pub async fn settings_get_global_layer(&self) -> Result<SettingsLayerPayload, ApiError> {
        self.watch_settings_paths(None);
        global_layer_payload()
    }

    pub async fn settings_patch_global_section(
        &self,
        input: PatchSettingsSectionInput,
    ) -> Result<SettingsLayerPayload, ApiError> {
        patch_global_section(&input.section, input.value)?;
        let payload = global_layer_payload()?;
        self.emit_settings_changed(SettingsLayerKind::Global, payload.diagnostics.clone())?;
        Ok(payload)
    }

    pub async fn settings_create_global_file(&self) -> Result<SettingsFileResult, ApiError> {
        let path = ensure_global_settings_file()?;
        Ok(SettingsFileResult {
            path: path.to_string_lossy().into_owned(),
        })
    }

    pub async fn settings_open_folder_file(
        &self,
        input: FolderSettingsFileInput,
    ) -> Result<SettingsFileResult, ApiError> {
        let path = ensure_folder_settings_file(PathBuf::from(input.folder_path))?;
        Ok(SettingsFileResult {
            path: path.to_string_lossy().into_owned(),
        })
    }

    pub async fn settings_set_window(
        &self,
        settings: WindowSettings,
    ) -> Result<WindowSettings, ApiError> {
        self.patch_and_emit_global_section("window", &settings)?;
        Ok(settings)
    }

    pub async fn settings_set_terminal(
        &self,
        settings: TerminalSettings,
    ) -> Result<TerminalSettings, ApiError> {
        self.patch_and_emit_global_section("terminal", &settings)?;
        Ok(settings)
    }

    pub async fn settings_set_nix(&self, settings: NixSettings) -> Result<NixSettings, ApiError> {
        self.patch_and_emit_global_section("nix", &settings)?;
        Ok(settings)
    }

    pub async fn settings_set_formatting(
        &self,
        formatting: FormattingSettings,
    ) -> Result<FormattingSettings, ApiError> {
        self.patch_and_emit_global_section("formatting", &formatting)?;
        Ok(formatting)
    }

    pub async fn settings_set_provider_config(
        &self,
        config: SaveProviderConfigInput,
    ) -> Result<ProviderConfigRecord, ApiError> {
        let record = ProviderConfigRecord {
            provider_id: config.provider_id,
            enabled: config.enabled,
            binary_path_override: config.binary_path_override,
            extra_args: config.extra_args,
        };
        patch_global_provider(&record)?;
        let diagnostics = global_layer_payload()?.diagnostics;
        self.emit_settings_changed(SettingsLayerKind::Global, diagnostics)?;
        Ok(record)
    }

    fn patch_and_emit_global_section<T: Serialize>(
        &self,
        section: &str,
        settings: &T,
    ) -> Result<(), ApiError> {
        patch_global_section(
            section,
            serde_json::to_value(settings)
                .map_err(|error| ApiError::Internal(error.to_string()))?,
        )?;
        let diagnostics = global_layer_payload()?.diagnostics;
        self.emit_settings_changed(SettingsLayerKind::Global, diagnostics)
    }

    fn watch_settings_paths(&self, folder_path: Option<&Path>) {
        let generation = Arc::clone(&self.settings_generation);
        if let Err(error) = self
            .settings_watchers
            .watch_global(Arc::clone(&self.events), generation)
        {
            tracing::warn!("settings global watcher failed: {error}");
        }

        if let Some(folder_path) = folder_path {
            let generation = Arc::clone(&self.settings_generation);
            if let Err(error) = self.settings_watchers.watch_folder(
                Arc::clone(&self.events),
                folder_path,
                generation,
            ) {
                tracing::warn!("settings folder watcher failed: {error}");
            }
        }
    }

    fn emit_settings_changed(
        &self,
        layer: SettingsLayerKind,
        diagnostics: Vec<SettingsDiagnostic>,
    ) -> Result<(), ApiError> {
        let generation = {
            let mut generation = self.settings_generation.lock();
            *generation += 1;
            *generation
        };
        (self.events)(HostEvent::SettingsChanged(SettingsChangedEvent {
            layer,
            folder_path: None,
            generation,
            diagnostics,
        }))
        .map_err(|error| ApiError::Internal(format!("Failed to emit settings-changed: {error}")))
    }
}

fn global_layer_payload() -> Result<SettingsLayerPayload, ApiError> {
    let path = SettingsService::global_settings_path().map_err(ApiError::Internal)?;
    Ok(layer_payload(
        crate::services::settings_resolution::load_settings_layer(SettingsLayerKind::Global, path),
    ))
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
        } => SettingsLayerPayload {
            path: path.to_string_lossy().into_owned(),
            loaded: false,
            value: json!({}),
            diagnostics: vec![parse_error_diagnostic(layer, &path, message)],
        },
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
    let providers_value = Value::Object(std::mem::take(providers));
    let patched = patch_top_level_section(&original, "providers", &providers_value)
        .map_err(ApiError::Internal)?;
    std::fs::write(path, patched).map_err(|error| {
        ApiError::Io(format!(
            "Failed to write settings file {}: {}",
            path.display(),
            error
        ))
    })
}

pub(crate) fn ensure_object_property<'a>(
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
    if matches!(
        section,
        "window" | "terminal" | "nix" | "explorer" | "formatting" | "providers" | "lsp"
    ) {
        Ok(())
    } else {
        Err(ApiError::InvalidArgument(format!(
            "Unsupported settings section `{section}`"
        )))
    }
}

pub(crate) fn ensure_global_settings_file() -> Result<PathBuf, ApiError> {
    let path = SettingsService::ensure_global_settings_parent().map_err(ApiError::Internal)?;
    ensure_file_exists(&path)?;
    Ok(path)
}

fn ensure_folder_settings_file(folder_path: PathBuf) -> Result<PathBuf, ApiError> {
    let path =
        SettingsService::ensure_folder_settings_parent(&folder_path).map_err(ApiError::Io)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use sworm_protocol::settings::{EffectiveSettings, ProviderSettings};
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sworm-settings-commands-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn validates_known_top_level_sections() {
        for section in [
            "window",
            "terminal",
            "nix",
            "explorer",
            "formatting",
            "providers",
            "lsp",
        ] {
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
    fn ensure_folder_settings_file_creates_parent_and_file() {
        let root = temp_root("folder-file");
        fs::create_dir_all(&root).expect("folder root created");
        let path = ensure_folder_settings_file(root.clone()).expect("settings file created");
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
            "{\n  \"future_top\": true,\n  \"terminal\": { \"font_size\": 13 }\n}\n",
        )
        .expect("settings file written");
        patch_section_file(&path, "terminal", json!({ "font_size": 16 })).expect("section patched");
        let patched = fs::read_to_string(&path).expect("patched file read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(parsed["terminal"]["font_size"], json!(16));
        fs::remove_dir_all(root).expect("temp dir removed");
    }

    #[test]
    fn patch_provider_file_merges_one_provider_into_providers_section() {
        let root = temp_root("patch-provider");
        fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        fs::write(
            &path,
            "{\n  \"providers\": {\n    \"codex\": { \"enabled\": false }\n  }\n}\n",
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
