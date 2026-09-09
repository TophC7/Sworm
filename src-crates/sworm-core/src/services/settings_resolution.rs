use crate::services::{
    builtins::BuiltinCatalogService,
    settings::{SettingsJsoncLayer, SettingsService},
};
use jsonschema::{error::ValidationErrorKind, Validator};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
use sworm_protocol::settings::{
    is_global_only_pointer, settings_layer_schema, EffectiveSettings, LspServerConfigRecord,
    LspServerSettings, ProviderConfigRecord, ProviderSettings, SettingsDiagnostic,
    SettingsDiagnosticCode, SettingsDiagnosticSeverity, SettingsLayerKind,
};

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsLayerLoad {
    Loaded(SettingsJsoncLayer),
    Invalid {
        layer: SettingsLayerKind,
        path: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSettings {
    pub settings: EffectiveSettings,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

pub fn resolve_effective_settings_for_folder_path(
    folder_path: Option<&Path>,
) -> Result<ResolvedSettings, String> {
    let lsp_server_ids = BuiltinCatalogService::list_server_definitions()?
        .into_iter()
        .map(|server| server.server_definition_id)
        .collect::<Vec<_>>();
    let global_path = SettingsService::global_settings_path()?;
    let global = load_settings_layer(SettingsLayerKind::Global, global_path);
    let folder = folder_path.map(|folder_path| {
        load_settings_layer(
            SettingsLayerKind::Folder,
            SettingsService::folder_settings_path(folder_path),
        )
    });

    Ok(resolve_effective_settings(global, folder, &lsp_server_ids))
}

pub fn provider_binary_overrides(settings: &EffectiveSettings) -> HashMap<String, String> {
    settings
        .providers
        .iter()
        .filter_map(|(provider_id, config)| {
            config
                .binary_path_override
                .as_ref()
                .filter(|path| !path.trim().is_empty())
                .map(|path| (provider_id.clone(), path.clone()))
        })
        .collect()
}

pub fn lsp_config_record(settings: &EffectiveSettings, server_id: &str) -> LspServerConfigRecord {
    let config = settings
        .lsp
        .servers
        .get(server_id)
        .cloned()
        .unwrap_or_else(LspServerSettings::default);
    LspServerConfigRecord {
        server_definition_id: server_id.to_string(),
        enabled: config.enabled,
        binary_path_override: config.binary_path_override,
        runtime_path_override: config.runtime_path_override,
        runtime_args: config.runtime_args,
        extra_args: config.extra_args,
        trace: config.trace,
        settings: config.settings,
    }
}

pub fn provider_config_record(
    settings: &EffectiveSettings,
    provider_id: &str,
) -> ProviderConfigRecord {
    let config = settings
        .providers
        .get(provider_id)
        .cloned()
        .unwrap_or_else(ProviderSettings::default);
    ProviderConfigRecord {
        provider_id: provider_id.to_string(),
        enabled: config.enabled,
        binary_path_override: config.binary_path_override,
        extra_args: config.extra_args,
    }
}

pub(crate) fn load_settings_layer(layer: SettingsLayerKind, path: PathBuf) -> SettingsLayerLoad {
    match SettingsService::read_jsonc_layer_or_empty(&path) {
        Ok(layer) => SettingsLayerLoad::Loaded(layer),
        Err(message) => SettingsLayerLoad::Invalid {
            layer,
            path,
            message,
        },
    }
}

pub fn parse_error_diagnostic(
    layer: SettingsLayerKind,
    path: &Path,
    message: String,
) -> SettingsDiagnostic {
    SettingsDiagnostic {
        layer,
        path: path.to_string_lossy().into_owned(),
        pointer: String::new(),
        code: SettingsDiagnosticCode::ParseError,
        severity: SettingsDiagnosticSeverity::Error,
        message,
    }
}

/// Resolves defaults <- global <- folder. Each layer is validated against the
/// schema derived from `EffectiveSettings`; rejected nodes are stripped and
/// reported, so one bad field never poisons its siblings or the other layer.
pub fn resolve_effective_settings(
    global: SettingsLayerLoad,
    folder: Option<SettingsLayerLoad>,
    lsp_server_ids: &[String],
) -> ResolvedSettings {
    // ponytail: rebuild per call; OnceLock<Validator> keyed by lsp ids if profiling shows cost
    let global_schema = settings_layer_schema(SettingsLayerKind::Global, lsp_server_ids);
    let global_validator =
        jsonschema::validator_for(&global_schema).expect("global model schema is valid");
    let mut diagnostics = Vec::new();
    let mut merged = serde_json::to_value(EffectiveSettings::with_lsp_server_ids(lsp_server_ids))
        .expect("default settings serialize");

    if let Some(value) = validate_layer(
        &global_validator,
        SettingsLayerKind::Global,
        global,
        &mut diagnostics,
    ) {
        merge_layer(&mut merged, value);
    }

    if let Some(folder_load) = folder {
        let folder_schema = settings_layer_schema(SettingsLayerKind::Folder, lsp_server_ids);
        let folder_validator =
            jsonschema::validator_for(&folder_schema).expect("folder model schema is valid");
        if let Some(value) = validate_layer(
            &folder_validator,
            SettingsLayerKind::Folder,
            folder_load,
            &mut diagnostics,
        ) {
            merge_layer(&mut merged, value);
        }
    }

    let settings = serde_json::from_value(merged).unwrap_or_else(|error| {
        // Bug guard: validation already stripped everything the model rejects.
        diagnostics.push(parse_error_diagnostic(
            SettingsLayerKind::Global,
            Path::new(""),
            format!("Settings model rejected validated layers: {error}"),
        ));
        EffectiveSettings::with_lsp_server_ids(lsp_server_ids)
    });

    ResolvedSettings {
        settings,
        diagnostics,
    }
}

/// Validates one layer, appending a diagnostic per rejected node and removing
/// those nodes from the returned value. `None` means the layer contributes
/// nothing (missing file, parse failure, or a non-object root).
fn validate_layer(
    validator: &Validator,
    kind: SettingsLayerKind,
    load: SettingsLayerLoad,
    diagnostics: &mut Vec<SettingsDiagnostic>,
) -> Option<Value> {
    let layer = match load {
        SettingsLayerLoad::Invalid {
            layer,
            path,
            message,
        } => {
            diagnostics.push(parse_error_diagnostic(layer, &path, message));
            return None;
        }
        SettingsLayerLoad::Loaded(layer) if layer.loaded => layer,
        SettingsLayerLoad::Loaded(_) => return None,
    };

    let mut rejected: Vec<(String, SettingsDiagnosticCode, String)> = Vec::new();
    for error in validator.iter_errors(&layer.value) {
        let at = error.instance_path().as_str();
        match error.kind() {
            ValidationErrorKind::AdditionalProperties { unexpected } => {
                rejected.extend(unexpected.iter().map(|key| {
                    let pointer = format!("{at}{}", json_pointer(&[key]));
                    let message = if kind == SettingsLayerKind::Folder
                        && is_global_only_pointer(&pointer)
                    {
                        format!(
                            "Setting `{key}` is window-scoped and can only be configured in global settings"
                        )
                    } else {
                        format!("Unknown setting `{key}`")
                    };
                    (pointer, SettingsDiagnosticCode::UnknownKey, message)
                }));
            }
            // Rejected map key: providers and lsp.servers are keyed by runtime
            // catalog ids, pinned in the schema as `propertyNames`.
            ValidationErrorKind::PropertyNames { error: inner } => {
                let key = inner.instance().as_str().unwrap_or_default().to_string();
                let noun = if at == "/providers" {
                    "provider"
                } else {
                    "LSP server"
                };
                rejected.push((
                    format!("{at}{}", json_pointer(&[&key])),
                    SettingsDiagnosticCode::UnknownKey,
                    format!("Unknown {noun} `{key}`"),
                ));
            }
            _ => rejected.push((
                at.to_string(),
                SettingsDiagnosticCode::InvalidValue,
                error.to_string(),
            )),
        }
    }

    let mut seen = HashSet::new();
    rejected.retain(|(pointer, code, _)| seen.insert((pointer.clone(), *code)));

    let path = layer.path.to_string_lossy().into_owned();
    let root_rejected = rejected.iter().any(|(pointer, ..)| pointer.is_empty());
    let mut value = layer.value;
    for (pointer, code, message) in rejected {
        remove_at(&mut value, &pointer);
        diagnostics.push(SettingsDiagnostic {
            layer: kind,
            path: path.clone(),
            severity: if pointer.is_empty() {
                SettingsDiagnosticSeverity::Error
            } else {
                SettingsDiagnosticSeverity::Warning
            },
            pointer,
            code,
            message,
        });
    }

    (!root_rejected).then_some(value)
}

/// Drops the node at `pointer`. An invalid element invalidates the list that
/// holds it, so the walk stops at the deepest object ancestor: an error at
/// `/providers/codex/extra_args/1` removes `extra_args`.
fn remove_at(value: &mut Value, pointer: &str) {
    let segments = pointer
        .split('/')
        .skip(1)
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .collect::<Vec<_>>();
    let mut node = value;
    for (index, segment) in segments.iter().enumerate() {
        let Some(object) = node.as_object_mut() else {
            return;
        };
        if index + 1 == segments.len() || !object.get(segment).is_some_and(Value::is_object) {
            object.remove(segment);
            return;
        }
        node = object.get_mut(segment).expect("key checked above");
    }
}

/// Object + object merges key by key; anything else replaces, so arrays and
/// explicit `null`s override the layer below.
fn merge_layer(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_layer(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn json_pointer(parts: &[&str]) -> String {
    let mut pointer = String::new();
    for part in parts {
        pointer.push('/');
        pointer.push_str(&part.replace('~', "~0").replace('/', "~1"));
    }
    pointer
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sworm_protocol::settings::{FormatterSelection, TabBeamPosition};

    fn loaded(layer: SettingsLayerKind, name: &str, value: Value) -> SettingsLayerLoad {
        SettingsLayerLoad::Loaded(SettingsJsoncLayer {
            path: PathBuf::from(format!("/{layer:?}/{name}.jsonc")),
            loaded: true,
            value,
        })
    }

    fn missing(layer: SettingsLayerKind, name: &str) -> SettingsLayerLoad {
        SettingsLayerLoad::Loaded(SettingsJsoncLayer {
            path: PathBuf::from(format!("/{layer:?}/{name}.jsonc")),
            loaded: false,
            value: json!({}),
        })
    }

    fn invalid(layer: SettingsLayerKind, name: &str) -> SettingsLayerLoad {
        SettingsLayerLoad::Invalid {
            layer,
            path: PathBuf::from(format!("/{layer:?}/{name}.jsonc")),
            message: "parse exploded".to_string(),
        }
    }

    fn resolve(global: SettingsLayerLoad, project: Option<SettingsLayerLoad>) -> ResolvedSettings {
        resolve_effective_settings(global, project, &["dev.sworm.vtsls::vtsls".to_string()])
    }

    #[test]
    fn missing_layers_resolve_defaults() {
        let resolved = resolve(missing(SettingsLayerKind::Global, "global"), None);

        assert_eq!(resolved.settings.terminal.font_size, 13);
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn valid_global_overrides_defaults() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "terminal": { "font_size": 15 } }),
            ),
            None,
        );

        assert_eq!(resolved.settings.terminal.font_size, 15);
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn valid_folder_overrides_global() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "terminal": { "font_size": 15 } }),
            ),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({ "terminal": { "font_size": 17 } }),
            )),
        );

        assert_eq!(resolved.settings.terminal.font_size, 17);
    }

    #[test]
    fn window_tab_beam_position_round_trips_in_global() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "window": { "tab_beam_position": "bottom" } }),
            ),
            None,
        );

        assert_eq!(
            resolved.settings.window.tab_beam_position,
            TabBeamPosition::Bottom
        );
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn folder_layer_rejects_window_settings_with_window_scoped_diagnostic() {
        let resolved = resolve(
            loaded(SettingsLayerKind::Global, "global", json!({})),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({ "window": { "tab_beam_position": "bottom" } }),
            )),
        );

        assert_eq!(
            resolved.settings.window.tab_beam_position,
            TabBeamPosition::Top
        );
        assert_eq!(resolved.diagnostics.len(), 1);
        let diag = &resolved.diagnostics[0];
        assert_eq!(diag.code, SettingsDiagnosticCode::UnknownKey);
        assert_eq!(diag.pointer, "/window");
        assert!(diag.message.contains("window-scoped"));
    }

    #[test]
    fn invalid_global_fails_only_global_layer() {
        let resolved = resolve(
            invalid(SettingsLayerKind::Global, "global"),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({ "terminal": { "font_size": 18 } }),
            )),
        );

        assert_eq!(resolved.settings.terminal.font_size, 18);
        assert_eq!(
            resolved.diagnostics[0].code,
            SettingsDiagnosticCode::ParseError
        );
        assert_eq!(resolved.diagnostics[0].layer, SettingsLayerKind::Global);
    }

    #[test]
    fn invalid_folder_fails_only_folder_layer() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "terminal": { "font_size": 16 } }),
            ),
            Some(invalid(SettingsLayerKind::Folder, "folder")),
        );

        assert_eq!(resolved.settings.terminal.font_size, 16);
        assert_eq!(
            resolved.diagnostics[0].code,
            SettingsDiagnosticCode::ParseError
        );
        assert_eq!(resolved.diagnostics[0].layer, SettingsLayerKind::Folder);
    }

    #[test]
    fn object_settings_merge_recursively_and_arrays_replace() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({
                    "providers": {
                        "claude_code": { "extra_args": ["--global"] }
                    },
                    "lsp": {
                        "servers": {
                            "dev.sworm.vtsls::vtsls": {
                                "settings": {
                                    "typescript": { "preferences": { "importModuleSpecifier": "relative" } },
                                    "array": ["global"]
                                }
                            }
                        }
                    }
                }),
            ),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({
                    "providers": {
                        "claude_code": { "extra_args": ["--project"] }
                    },
                    "lsp": {
                        "servers": {
                            "dev.sworm.vtsls::vtsls": {
                                "settings": {
                                    "typescript": { "preferences": { "quoteStyle": "single" } },
                                    "array": ["project"]
                                }
                            }
                        }
                    }
                }),
            )),
        );

        assert_eq!(
            resolved.settings.providers["claude_code"].extra_args,
            vec!["--project"]
        );
        let lsp_settings = resolved.settings.lsp.servers["dev.sworm.vtsls::vtsls"]
            .settings
            .as_ref()
            .expect("lsp settings merged");
        assert_eq!(
            lsp_settings["typescript"]["preferences"]["importModuleSpecifier"],
            json!("relative")
        );
        assert_eq!(
            lsp_settings["typescript"]["preferences"]["quoteStyle"],
            json!("single")
        );
        assert_eq!(lsp_settings["array"], json!(["project"]));
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn nullable_null_applies_and_invalid_non_nullable_null_is_ignored() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({
                    "providers": {
                        "claude_code": {
                            "enabled": null,
                            "binary_path_override": "/bin/claude"
                        }
                    }
                }),
            ),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({
                    "providers": {
                        "claude_code": {
                            "binary_path_override": null
                        }
                    }
                }),
            )),
        );

        let provider = &resolved.settings.providers["claude_code"];
        assert!(provider.enabled);
        assert_eq!(provider.binary_path_override, None);
        assert!(resolved.diagnostics.iter().any(|diagnostic| diagnostic.code
            == SettingsDiagnosticCode::InvalidValue
            && diagnostic.pointer == "/providers/claude_code/enabled"));
    }

    #[test]
    fn unknown_provider_and_lsp_ids_emit_diagnostics_and_are_ignored() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({
                    "providers": { "missing_provider": { "enabled": false } },
                    "lsp": { "servers": { "missing::server": { "enabled": false } } }
                }),
            ),
            None,
        );

        assert!(resolved.settings.providers["claude_code"].enabled);
        assert!(resolved.settings.lsp.servers["dev.sworm.vtsls::vtsls"].enabled);
        assert!(!resolved.settings.providers.contains_key("missing_provider"));
        assert!(!resolved
            .settings
            .lsp
            .servers
            .contains_key("missing::server"));
        for pointer in [
            "/providers/missing_provider",
            "/lsp/servers/missing::server",
        ] {
            assert!(
                resolved.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == SettingsDiagnosticCode::UnknownKey
                        && diagnostic.pointer == pointer
                }),
                "diagnostic for {pointer}"
            );
        }
    }

    #[test]
    fn semantic_errors_ignore_bad_field_and_apply_rest_of_layer() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({
                    "terminal": {
                        "font_size": "big",
                        "font_family": "Iosevka"
                    },
                    "formatting": {
                        "json": { "formatter": "weird" },
                        "nix": { "formatter": "disabled" }
                    }
                }),
            ),
            None,
        );

        assert_eq!(resolved.settings.terminal.font_size, 13);
        assert_eq!(resolved.settings.terminal.font_family, "Iosevka");
        assert_eq!(
            resolved.settings.formatting.json.formatter,
            FormatterSelection::Biome
        );
        assert_eq!(
            resolved.settings.formatting.nix.formatter,
            FormatterSelection::Disabled
        );
        for pointer in ["/terminal/font_size", "/formatting/json/formatter"] {
            assert!(
                resolved.diagnostics.iter().any(|diagnostic| {
                    diagnostic.code == SettingsDiagnosticCode::InvalidValue
                        && diagnostic.pointer == pointer
                }),
                "diagnostic for {pointer}"
            );
        }
    }

    #[test]
    fn explorer_exclude_merges_with_defaults() {
        let resolved = resolve(
            loaded(SettingsLayerKind::Global, "global", json!({})),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({
                    "explorer": {
                        "exclude": { "**/vendor": true, "**/.git": false },
                        "exclude_gitignore": true,
                        "compact_folders": false
                    }
                }),
            )),
        );

        let exclude = &resolved.settings.explorer.exclude;
        assert!(exclude["**/vendor"]);
        assert!(!exclude["**/.git"]);
        assert!(exclude["**/.DS_Store"]);
        assert!(resolved.settings.explorer.exclude_gitignore);
        assert!(!resolved.settings.explorer.compact_folders);
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn explorer_unknown_key_emits_diagnostic() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "explorer": { "excludeGitIgnore": true } }),
            ),
            None,
        );

        assert!(!resolved.settings.explorer.exclude_gitignore);
        assert!(resolved.diagnostics.iter().any(|diagnostic| diagnostic.code
            == SettingsDiagnosticCode::UnknownKey
            && diagnostic.pointer == "/explorer/excludeGitIgnore"));
    }

    #[test]
    fn explorer_non_bool_exclude_value_is_ignored_with_diagnostic() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({
                    "explorer": {
                        "exclude": { "**/vendor": "yes", "**/gen": true },
                        "compact_folders": false
                    }
                }),
            ),
            None,
        );

        let exclude = &resolved.settings.explorer.exclude;
        assert!(exclude["**/gen"]);
        assert!(!exclude.contains_key("**/vendor"));
        assert!(!resolved.settings.explorer.compact_folders);
        assert!(resolved.diagnostics.iter().any(|diagnostic| diagnostic.code
            == SettingsDiagnosticCode::InvalidValue
            && diagnostic.pointer == "/explorer/exclude/**~1vendor"));
    }
}
