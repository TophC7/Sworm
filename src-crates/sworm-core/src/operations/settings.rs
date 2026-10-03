use crate::{
    errors::ApiError,
    services::{
        settings::SettingsService,
        settings_resolution::{
            parse_error_diagnostic, provider_binary_overrides, provider_config_record,
            resolve_effective_settings_for_folder_path, SettingsLayerLoad,
        },
    },
    Host,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use sworm_protocol::settings::{
    EffectiveSettingsPayload, FormattingSettings, NixSettings, PatchSettingsSectionInput,
    ProviderConfigRecord, ProviderSettingsEntry, SettingsFileResult, SettingsLayerKind,
    SettingsLayerPayload, SettingsPayload, TerminalSettings, WindowSettings,
};

impl Host {
    pub fn settings_get(&self) -> Result<SettingsPayload, ApiError> {
        self.watch_settings_paths(None);
        let resolved =
            resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
        let overrides = provider_binary_overrides(&resolved.settings);
        let providers = self
            .providers
            .detect_all(self.env.path(), &overrides, Some(&self.env.detected_shell))
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

    pub fn settings_get_effective(
        &self,
        folder_path: Option<String>,
    ) -> Result<EffectiveSettingsPayload, ApiError> {
        // Remote workspaces have no local folder layer: the daemon owns their
        // files, and the desktop's own settings still govern this window.
        let folder_path = folder_path
            .filter(|path| !path.starts_with("sworm://"))
            .map(PathBuf::from);
        self.watch_settings_paths(folder_path.as_deref());
        let resolved = resolve_effective_settings_for_folder_path(folder_path.as_deref())
            .map_err(ApiError::Internal)?;
        Ok(EffectiveSettingsPayload {
            settings: resolved.settings,
            diagnostics: resolved.diagnostics,
        })
    }

    pub fn settings_get_global_layer(&self) -> Result<SettingsLayerPayload, ApiError> {
        self.watch_settings_paths(None);
        global_layer_payload()
    }

    pub fn settings_patch_global_section(
        &self,
        input: PatchSettingsSectionInput,
    ) -> Result<SettingsLayerPayload, ApiError> {
        self.settings_update_global_section(&input.section, |_| Ok(input.value))
    }

    /// Atomically update a section against its latest committed value.
    pub fn settings_update_global_section(
        &self,
        section: &str,
        update: impl FnOnce(Value) -> Result<Value, ApiError>,
    ) -> Result<SettingsLayerPayload, ApiError> {
        let path = SettingsService::global_settings_path().map_err(ApiError::Internal)?;
        let payload = SettingsService::update_section(&path, section, update)?;
        self.settings_watchers
            .notify(SettingsLayerKind::Global, None);
        Ok(payload)
    }

    pub fn settings_create_global_file(&self) -> Result<SettingsFileResult, ApiError> {
        let path = SettingsService::global_settings_path().map_err(ApiError::Internal)?;
        SettingsService::ensure_file(&path)?;
        Ok(SettingsFileResult {
            path: path.to_string_lossy().into_owned(),
        })
    }

    pub fn settings_open_folder_file(
        &self,
        folder_path: String,
    ) -> Result<SettingsFileResult, ApiError> {
        let path = SettingsService::folder_settings_path(Path::new(&folder_path));
        SettingsService::ensure_file(&path)?;
        Ok(SettingsFileResult {
            path: path.to_string_lossy().into_owned(),
        })
    }

    pub fn settings_set_window(
        &self,
        settings: WindowSettings,
    ) -> Result<WindowSettings, ApiError> {
        let value = serde_json::to_value(&settings)
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        self.settings_update_global_section("window", |_| Ok(value))?;
        Ok(settings)
    }

    pub fn settings_set_terminal(
        &self,
        settings: TerminalSettings,
    ) -> Result<TerminalSettings, ApiError> {
        let value = serde_json::to_value(&settings)
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        self.settings_update_global_section("terminal", |_| Ok(value))?;
        Ok(settings)
    }

    pub fn settings_set_nix(&self, settings: NixSettings) -> Result<NixSettings, ApiError> {
        let value = serde_json::to_value(&settings)
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        self.settings_update_global_section("nix", |_| Ok(value))?;
        Ok(settings)
    }

    pub fn settings_set_formatting(
        &self,
        formatting: FormattingSettings,
    ) -> Result<FormattingSettings, ApiError> {
        let value = serde_json::to_value(&formatting)
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        self.settings_update_global_section("formatting", |_| Ok(value))?;
        Ok(formatting)
    }

    pub fn settings_set_provider_config(
        &self,
        config: ProviderConfigRecord,
    ) -> Result<ProviderConfigRecord, ApiError> {
        self.settings_update_global_section("providers", |providers| {
            let mut root = json!({ "providers": providers });
            let providers = SettingsService::object_property(&mut root, "providers")?;
            providers.insert(
                config.provider_id.clone(),
                json!({
                    "enabled": config.enabled,
                    "binary_path_override": config.binary_path_override,
                    "extra_args": config.extra_args,
                }),
            );
            Ok(Value::Object(std::mem::take(providers)))
        })?;
        Ok(config)
    }

    pub fn settings_paths_watched(&self, folder_path: &Path) -> bool {
        self.settings_watchers.is_watching(folder_path)
    }

    pub fn watch_settings_paths(&self, folder_path: Option<&Path>) {
        if let Err(error) = self.settings_watchers.watch_global() {
            tracing::warn!("settings global watcher failed: {error}");
        }

        if let Some(folder_path) = folder_path {
            if let Err(error) = self.settings_watchers.watch_folder(folder_path) {
                tracing::warn!("settings folder watcher failed: {error}");
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use sworm_protocol::settings::{EffectiveSettings, ProviderSettings};

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
}
