use crate::models::settings::{
    self, EffectiveSettings, FormatterSelection, LspServerConfigRecord, LspServerSettings,
    LspTraceLevel, ProviderConfigRecord, ProviderSettings, SettingsDiagnostic,
    SettingsDiagnosticCode, SettingsDiagnosticSeverity, SettingsLayerKind,
};
use crate::services::{
    builtins::BuiltinCatalogService,
    settings::{SettingsJsoncLayer, SettingsService},
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
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

    Ok(resolve_effective_settings(global, folder, lsp_server_ids))
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

pub fn resolve_effective_settings<I, S>(
    global: SettingsLayerLoad,
    folder: Option<SettingsLayerLoad>,
    valid_lsp_server_ids: I,
) -> ResolvedSettings
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let lsp_server_ids = valid_lsp_server_ids
        .into_iter()
        .map(|id| id.as_ref().to_string())
        .collect::<Vec<_>>();
    let valid_lsp_server_ids = lsp_server_ids.iter().cloned().collect::<BTreeSet<_>>();
    let mut ctx = ResolutionContext {
        settings: EffectiveSettings::with_lsp_server_ids(&lsp_server_ids),
        diagnostics: Vec::new(),
        valid_lsp_server_ids,
    };

    ctx.apply_layer(SettingsLayerKind::Global, global);
    if let Some(folder) = folder {
        ctx.apply_layer(SettingsLayerKind::Folder, folder);
    }

    ResolvedSettings {
        settings: ctx.settings,
        diagnostics: ctx.diagnostics,
    }
}

struct ResolutionContext {
    settings: EffectiveSettings,
    diagnostics: Vec<SettingsDiagnostic>,
    valid_lsp_server_ids: BTreeSet<String>,
}

impl ResolutionContext {
    fn apply_layer(&mut self, layer_kind: SettingsLayerKind, layer: SettingsLayerLoad) {
        let layer = match layer {
            SettingsLayerLoad::Loaded(layer) => layer,
            SettingsLayerLoad::Invalid {
                layer,
                path,
                message,
            } => {
                self.push_diagnostic(
                    layer,
                    path.to_string_lossy().into_owned(),
                    "",
                    SettingsDiagnosticCode::ParseError,
                    SettingsDiagnosticSeverity::Error,
                    message,
                );
                return;
            }
        };

        if !layer.loaded {
            return;
        }

        let path = layer.path.to_string_lossy().into_owned();
        let Some(root) = layer.value.as_object() else {
            self.push_diagnostic(
                layer_kind,
                path,
                "",
                SettingsDiagnosticCode::TypeError,
                SettingsDiagnosticSeverity::Error,
                "Settings layer must be a JSON object".to_string(),
            );
            return;
        };

        self.apply_root_object(layer_kind, &path, root);
    }

    fn apply_root_object(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        root: &Map<String, Value>,
    ) {
        for key in root.keys() {
            if !matches!(
                key.as_str(),
                "general" | "explorer" | "formatting" | "providers" | "lsp"
            ) {
                self.unknown_key(layer, path, &[key]);
            }
        }

        if let Some(value) = root.get("general") {
            if let Some(object) = self.expect_object(layer, path, &["general"], value, false) {
                self.apply_general(layer, path, object);
            }
        }

        if let Some(value) = root.get("explorer") {
            if let Some(object) = self.expect_object(layer, path, &["explorer"], value, false) {
                self.apply_explorer(layer, path, object);
            }
        }

        if let Some(value) = root.get("formatting") {
            if let Some(object) = self.expect_object(layer, path, &["formatting"], value, false) {
                self.apply_formatting(layer, path, object);
            }
        }

        if let Some(value) = root.get("providers") {
            if let Some(object) = self.expect_object(layer, path, &["providers"], value, false) {
                self.apply_providers(layer, path, object);
            }
        }

        if let Some(value) = root.get("lsp") {
            if let Some(object) = self.expect_object(layer, path, &["lsp"], value, false) {
                self.apply_lsp(layer, path, object);
            }
        }
    }

    fn apply_general(&mut self, layer: SettingsLayerKind, path: &str, object: &Map<String, Value>) {
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "theme" | "terminal_font_family" | "terminal_font_size" | "nix_eval_timeout_secs"
            ) {
                self.unknown_key(layer, path, &["general", key]);
            }
        }

        if let Some(theme) = object.get("theme") {
            if let Some(value) =
                self.expect_string(layer, path, &["general", "theme"], theme, false)
            {
                self.settings.general.theme = value.to_string();
            }
        }
        if let Some(font_family) = object.get("terminal_font_family") {
            if let Some(value) = self.expect_string(
                layer,
                path,
                &["general", "terminal_font_family"],
                font_family,
                false,
            ) {
                self.settings.general.terminal_font_family = value.to_string();
            }
        }
        if let Some(font_size) = object.get("terminal_font_size") {
            if let Some(value) = self.expect_u64(
                layer,
                path,
                &["general", "terminal_font_size"],
                font_size,
                false,
            ) {
                match u16::try_from(value) {
                    Ok(value) => self.settings.general.terminal_font_size = value,
                    Err(_) => self.type_error(
                        layer,
                        path,
                        &["general", "terminal_font_size"],
                        "Expected integer between 0 and 65535",
                    ),
                }
            }
        }
        if let Some(timeout) = object.get("nix_eval_timeout_secs") {
            if let Some(value) = self.expect_u64(
                layer,
                path,
                &["general", "nix_eval_timeout_secs"],
                timeout,
                false,
            ) {
                self.settings.general.nix_eval_timeout_secs = value;
            }
        }
    }

    fn apply_explorer(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        object: &Map<String, Value>,
    ) {
        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "exclude" | "exclude_gitignore" | "compact_folders"
            ) {
                self.unknown_key(layer, path, &["explorer", key]);
            }
        }

        // A valid `exclude` object replaces the layer below it wholesale, so a
        // user who lists their own globs is not stuck with the defaults.
        if let Some(exclude) = object.get("exclude") {
            if let Some(object) =
                self.expect_object(layer, path, &["explorer", "exclude"], exclude, false)
            {
                let mut globs = BTreeMap::new();
                for (glob, value) in object {
                    match value.as_bool() {
                        Some(enabled) => {
                            globs.insert(glob.clone(), enabled);
                        }
                        None => self.type_error(
                            layer,
                            path,
                            &["explorer", "exclude", glob],
                            "Expected boolean",
                        ),
                    }
                }
                self.settings.explorer.exclude = globs;
            }
        }
        if let Some(exclude_gitignore) = object.get("exclude_gitignore") {
            if let Some(value) = self.expect_bool(
                layer,
                path,
                &["explorer", "exclude_gitignore"],
                exclude_gitignore,
                false,
            ) {
                self.settings.explorer.exclude_gitignore = value;
            }
        }
        if let Some(compact_folders) = object.get("compact_folders") {
            if let Some(value) = self.expect_bool(
                layer,
                path,
                &["explorer", "compact_folders"],
                compact_folders,
                false,
            ) {
                self.settings.explorer.compact_folders = value;
            }
        }
    }

    fn apply_formatting(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        object: &Map<String, Value>,
    ) {
        for key in object.keys() {
            if !matches!(key.as_str(), "javascript_typescript" | "json" | "nix") {
                self.unknown_key(layer, path, &["formatting", key]);
            }
        }

        for group in ["javascript_typescript", "json", "nix"] {
            let Some(value) = object.get(group) else {
                continue;
            };
            let Some(group_object) =
                self.expect_object(layer, path, &["formatting", group], value, false)
            else {
                continue;
            };

            for key in group_object.keys() {
                if key != "formatter" {
                    self.unknown_key(layer, path, &["formatting", group, key]);
                }
            }

            let Some(formatter) = group_object.get("formatter") else {
                continue;
            };
            let Some(formatter) = self.expect_string(
                layer,
                path,
                &["formatting", group, "formatter"],
                formatter,
                false,
            ) else {
                continue;
            };
            let Some(formatter) =
                self.parse_formatter(layer, path, &["formatting", group, "formatter"], formatter)
            else {
                continue;
            };

            match group {
                "javascript_typescript" => {
                    self.settings.formatting.javascript_typescript.formatter = formatter;
                }
                "json" => self.settings.formatting.json.formatter = formatter,
                "nix" => self.settings.formatting.nix.formatter = formatter,
                _ => unreachable!("known group"),
            }
        }
    }

    fn apply_providers(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        object: &Map<String, Value>,
    ) {
        for (provider_id, value) in object {
            if !settings::CANONICAL_PROVIDER_IDS
                .iter()
                .any(|known| known.to_string() == *provider_id)
            {
                self.push_diagnostic(
                    layer,
                    path.to_string(),
                    json_pointer(&["providers", provider_id]),
                    SettingsDiagnosticCode::UnknownProvider,
                    SettingsDiagnosticSeverity::Warning,
                    format!("Unknown provider ID `{provider_id}` ignored"),
                );
                continue;
            }

            let Some(provider) = self
                .settings
                .providers
                .get_mut(provider_id)
                .map(std::mem::take)
            else {
                continue;
            };
            let next = self.apply_provider(layer, path, provider_id, value, provider);
            self.settings.providers.insert(provider_id.clone(), next);
        }
    }

    fn apply_provider(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        provider_id: &str,
        value: &Value,
        mut provider: ProviderSettings,
    ) -> ProviderSettings {
        let Some(object) =
            self.expect_object(layer, path, &["providers", provider_id], value, false)
        else {
            return provider;
        };

        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "enabled" | "binary_path_override" | "extra_args"
            ) {
                self.unknown_key(layer, path, &["providers", provider_id, key]);
            }
        }

        if let Some(enabled) = object.get("enabled") {
            if let Some(value) = self.expect_bool(
                layer,
                path,
                &["providers", provider_id, "enabled"],
                enabled,
                false,
            ) {
                provider.enabled = value;
            }
        }
        if let Some(path_override) = object.get("binary_path_override") {
            if let Some(value) = self.expect_nullable_string(
                layer,
                path,
                &["providers", provider_id, "binary_path_override"],
                path_override,
            ) {
                provider.binary_path_override = value;
            }
        }
        if let Some(extra_args) = object.get("extra_args") {
            if let Some(value) = self.expect_string_array(
                layer,
                path,
                &["providers", provider_id, "extra_args"],
                extra_args,
                false,
            ) {
                provider.extra_args = value;
            }
        }

        provider
    }

    fn apply_lsp(&mut self, layer: SettingsLayerKind, path: &str, object: &Map<String, Value>) {
        for key in object.keys() {
            if key != "servers" {
                self.unknown_key(layer, path, &["lsp", key]);
            }
        }

        let Some(servers) = object.get("servers") else {
            return;
        };
        let Some(servers) = self.expect_object(layer, path, &["lsp", "servers"], servers, false)
        else {
            return;
        };

        for (server_id, value) in servers {
            if !self.valid_lsp_server_ids.contains(server_id) {
                self.push_diagnostic(
                    layer,
                    path.to_string(),
                    json_pointer(&["lsp", "servers", server_id]),
                    SettingsDiagnosticCode::UnknownLspServer,
                    SettingsDiagnosticSeverity::Warning,
                    format!("Unknown LSP server ID `{server_id}` ignored"),
                );
                continue;
            }

            let server = self
                .settings
                .lsp
                .servers
                .remove(server_id)
                .unwrap_or_default();
            let next = self.apply_lsp_server(layer, path, server_id, value, server);
            self.settings.lsp.servers.insert(server_id.clone(), next);
        }
    }

    fn apply_lsp_server(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        server_id: &str,
        value: &Value,
        mut server: LspServerSettings,
    ) -> LspServerSettings {
        let Some(object) =
            self.expect_object(layer, path, &["lsp", "servers", server_id], value, false)
        else {
            return server;
        };

        for key in object.keys() {
            if !matches!(
                key.as_str(),
                "enabled"
                    | "binary_path_override"
                    | "runtime_path_override"
                    | "runtime_args"
                    | "extra_args"
                    | "trace"
                    | "settings"
            ) {
                self.unknown_key(layer, path, &["lsp", "servers", server_id, key]);
            }
        }

        if let Some(enabled) = object.get("enabled") {
            if let Some(value) = self.expect_bool(
                layer,
                path,
                &["lsp", "servers", server_id, "enabled"],
                enabled,
                false,
            ) {
                server.enabled = value;
            }
        }
        if let Some(binary_path_override) = object.get("binary_path_override") {
            if let Some(value) = self.expect_nullable_string(
                layer,
                path,
                &["lsp", "servers", server_id, "binary_path_override"],
                binary_path_override,
            ) {
                server.binary_path_override = value;
            }
        }
        if let Some(runtime_path_override) = object.get("runtime_path_override") {
            if let Some(value) = self.expect_nullable_string(
                layer,
                path,
                &["lsp", "servers", server_id, "runtime_path_override"],
                runtime_path_override,
            ) {
                server.runtime_path_override = value;
            }
        }
        if let Some(runtime_args) = object.get("runtime_args") {
            if let Some(value) = self.expect_string_array(
                layer,
                path,
                &["lsp", "servers", server_id, "runtime_args"],
                runtime_args,
                false,
            ) {
                server.runtime_args = value;
            }
        }
        if let Some(extra_args) = object.get("extra_args") {
            if let Some(value) = self.expect_string_array(
                layer,
                path,
                &["lsp", "servers", server_id, "extra_args"],
                extra_args,
                false,
            ) {
                server.extra_args = value;
            }
        }
        if let Some(trace) = object.get("trace") {
            if let Some(value) = self.expect_string(
                layer,
                path,
                &["lsp", "servers", server_id, "trace"],
                trace,
                false,
            ) {
                if let Some(value) =
                    self.parse_trace(layer, path, &["lsp", "servers", server_id, "trace"], value)
                {
                    server.trace = value;
                }
            }
        }
        if let Some(settings) = object.get("settings") {
            if settings.is_null() {
                server.settings = None;
            } else {
                server.settings = Some(match server.settings.take() {
                    Some(existing) => merge_values(existing, settings.clone()),
                    None => settings.clone(),
                });
            }
        }

        server
    }

    fn expect_object<'a>(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &'a Value,
        nullable: bool,
    ) -> Option<&'a Map<String, Value>> {
        if value.is_null() {
            if nullable {
                return None;
            }
            self.invalid_null(layer, path, pointer);
            return None;
        }

        value.as_object().or_else(|| {
            self.type_error(layer, path, pointer, "Expected object");
            None
        })
    }

    fn expect_string<'a>(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &'a Value,
        nullable: bool,
    ) -> Option<&'a str> {
        if value.is_null() {
            if nullable {
                return None;
            }
            self.invalid_null(layer, path, pointer);
            return None;
        }

        value.as_str().or_else(|| {
            self.type_error(layer, path, pointer, "Expected string");
            None
        })
    }

    fn expect_nullable_string(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &Value,
    ) -> Option<Option<String>> {
        if value.is_null() {
            return Some(None);
        }

        self.expect_string(layer, path, pointer, value, false)
            .map(|value| Some(value.to_string()))
    }

    fn expect_bool(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &Value,
        nullable: bool,
    ) -> Option<bool> {
        if value.is_null() {
            if nullable {
                return None;
            }
            self.invalid_null(layer, path, pointer);
            return None;
        }

        value.as_bool().or_else(|| {
            self.type_error(layer, path, pointer, "Expected boolean");
            None
        })
    }

    fn expect_u64(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &Value,
        nullable: bool,
    ) -> Option<u64> {
        if value.is_null() {
            if nullable {
                return None;
            }
            self.invalid_null(layer, path, pointer);
            return None;
        }

        value.as_u64().or_else(|| {
            self.type_error(layer, path, pointer, "Expected non-negative integer");
            None
        })
    }

    fn expect_string_array(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &Value,
        nullable: bool,
    ) -> Option<Vec<String>> {
        if value.is_null() {
            if nullable {
                return None;
            }
            self.invalid_null(layer, path, pointer);
            return None;
        }

        let Some(values) = value.as_array() else {
            self.type_error(layer, path, pointer, "Expected array of strings");
            return None;
        };

        let mut strings = Vec::with_capacity(values.len());
        for (index, value) in values.iter().enumerate() {
            let Some(value) = value.as_str() else {
                self.type_error(
                    layer,
                    path,
                    &[pointer, &[&index.to_string()]].concat(),
                    "Expected string",
                );
                return None;
            };
            strings.push(value.to_string());
        }
        Some(strings)
    }

    fn parse_formatter(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &str,
    ) -> Option<FormatterSelection> {
        match value {
            "lsp" => Some(FormatterSelection::Lsp),
            "biome" => Some(FormatterSelection::Biome),
            "nixfmt" => Some(FormatterSelection::Nixfmt),
            "disabled" => Some(FormatterSelection::Disabled),
            _ => {
                self.push_diagnostic(
                    layer,
                    path.to_string(),
                    json_pointer(pointer),
                    SettingsDiagnosticCode::InvalidEnum,
                    SettingsDiagnosticSeverity::Warning,
                    format!("Invalid formatter `{value}` ignored"),
                );
                None
            }
        }
    }

    fn parse_trace(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        value: &str,
    ) -> Option<LspTraceLevel> {
        match value {
            "off" => Some(LspTraceLevel::Off),
            "messages" => Some(LspTraceLevel::Messages),
            "verbose" => Some(LspTraceLevel::Verbose),
            _ => {
                self.push_diagnostic(
                    layer,
                    path.to_string(),
                    json_pointer(pointer),
                    SettingsDiagnosticCode::InvalidEnum,
                    SettingsDiagnosticSeverity::Warning,
                    format!("Invalid LSP trace level `{value}` ignored"),
                );
                None
            }
        }
    }

    fn unknown_key(&mut self, layer: SettingsLayerKind, path: &str, pointer: &[&str]) {
        self.push_diagnostic(
            layer,
            path.to_string(),
            json_pointer(pointer),
            SettingsDiagnosticCode::UnknownKey,
            SettingsDiagnosticSeverity::Warning,
            format!(
                "Unknown settings key `{}` has no runtime effect",
                pointer.join(".")
            ),
        );
    }

    fn invalid_null(&mut self, layer: SettingsLayerKind, path: &str, pointer: &[&str]) {
        self.push_diagnostic(
            layer,
            path.to_string(),
            json_pointer(pointer),
            SettingsDiagnosticCode::InvalidNull,
            SettingsDiagnosticSeverity::Warning,
            "Null is not valid for this setting".to_string(),
        );
    }

    fn type_error(
        &mut self,
        layer: SettingsLayerKind,
        path: &str,
        pointer: &[&str],
        message: &str,
    ) {
        self.push_diagnostic(
            layer,
            path.to_string(),
            json_pointer(pointer),
            SettingsDiagnosticCode::TypeError,
            SettingsDiagnosticSeverity::Warning,
            message.to_string(),
        );
    }

    fn push_diagnostic(
        &mut self,
        layer: SettingsLayerKind,
        path: String,
        pointer: impl Into<String>,
        code: SettingsDiagnosticCode,
        severity: SettingsDiagnosticSeverity,
        message: String,
    ) {
        self.diagnostics.push(SettingsDiagnostic {
            layer,
            path,
            pointer: pointer.into(),
            code,
            severity,
            message,
        });
    }
}

fn merge_values(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                let merged = base
                    .remove(&key)
                    .map(|existing| merge_values(existing, value.clone()))
                    .unwrap_or(value);
                base.insert(key, merged);
            }
            Value::Object(base)
        }
        (_, overlay) => overlay,
    }
}

fn json_pointer(parts: &[&str]) -> String {
    if parts.is_empty() {
        return String::new();
    }

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
        resolve_effective_settings(global, project, ["dev.sworm.vtsls::vtsls"])
    }

    #[test]
    fn missing_layers_resolve_defaults() {
        let resolved = resolve(missing(SettingsLayerKind::Global, "global"), None);

        assert_eq!(resolved.settings.general.terminal_font_size, 13);
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn valid_global_overrides_defaults() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "general": { "terminal_font_size": 15 } }),
            ),
            None,
        );

        assert_eq!(resolved.settings.general.terminal_font_size, 15);
        assert!(resolved.diagnostics.is_empty());
    }

    #[test]
    fn valid_folder_overrides_global() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({ "general": { "terminal_font_size": 15 } }),
            ),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({ "general": { "terminal_font_size": 17 } }),
            )),
        );

        assert_eq!(resolved.settings.general.terminal_font_size, 17);
    }

    #[test]
    fn invalid_global_fails_only_global_layer() {
        let resolved = resolve(
            invalid(SettingsLayerKind::Global, "global"),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({ "general": { "terminal_font_size": 18 } }),
            )),
        );

        assert_eq!(resolved.settings.general.terminal_font_size, 18);
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
                json!({ "general": { "terminal_font_size": 16 } }),
            ),
            Some(invalid(SettingsLayerKind::Folder, "folder")),
        );

        assert_eq!(resolved.settings.general.terminal_font_size, 16);
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
            == SettingsDiagnosticCode::InvalidNull
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
        assert!(resolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SettingsDiagnosticCode::UnknownProvider));
        assert!(resolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SettingsDiagnosticCode::UnknownLspServer));
    }

    #[test]
    fn semantic_errors_ignore_bad_field_and_apply_rest_of_layer() {
        let resolved = resolve(
            loaded(
                SettingsLayerKind::Global,
                "global",
                json!({
                    "general": {
                        "terminal_font_size": "big",
                        "terminal_font_family": "Iosevka"
                    },
                    "formatting": {
                        "json": { "formatter": "weird" },
                        "nix": { "formatter": "disabled" }
                    }
                }),
            ),
            None,
        );

        assert_eq!(resolved.settings.general.terminal_font_size, 13);
        assert_eq!(resolved.settings.general.terminal_font_family, "Iosevka");
        assert_eq!(
            resolved.settings.formatting.json.formatter,
            FormatterSelection::Biome
        );
        assert_eq!(
            resolved.settings.formatting.nix.formatter,
            FormatterSelection::Disabled
        );
        assert!(resolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SettingsDiagnosticCode::TypeError));
        assert!(resolved
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == SettingsDiagnosticCode::InvalidEnum));
    }

    #[test]
    fn explorer_exclude_replaces_defaults_from_folder_layer() {
        let resolved = resolve(
            loaded(SettingsLayerKind::Global, "global", json!({})),
            Some(loaded(
                SettingsLayerKind::Folder,
                "folder",
                json!({
                    "explorer": {
                        "exclude": { "**/vendor": true },
                        "exclude_gitignore": true,
                        "compact_folders": false
                    }
                }),
            )),
        );

        assert_eq!(
            resolved.settings.explorer.exclude,
            BTreeMap::from([("**/vendor".to_string(), true)])
        );
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

        assert_eq!(
            resolved.settings.explorer.exclude,
            BTreeMap::from([("**/gen".to_string(), true)])
        );
        assert!(!resolved.settings.explorer.compact_folders);
        assert!(resolved.diagnostics.iter().any(|diagnostic| diagnostic.code
            == SettingsDiagnosticCode::TypeError
            && diagnostic.pointer == "/explorer/exclude/**~1vendor"));
    }
}
