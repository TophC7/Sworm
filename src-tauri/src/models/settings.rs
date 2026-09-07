use crate::models::provider::ProviderId;
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const SETTINGS_FILE_NAME: &str = "settings.jsonc";
pub const SHORTCUTS_FILE_NAME: &str = "shortcuts.jsonc";
pub const FOLDER_SETTINGS_PATH: &str = ".sworm/settings.jsonc";
pub const GLOBAL_SETTINGS_DIR_NAME: &str = "sworm";

/// Default timeout for `nix develop --command env -0` evaluations, in seconds.
/// 120s was too aggressive for cold stores pulling GUI deps (webkitgtk, gtk3,
/// rust toolchain); 600s leaves headroom while still catching true hangs.
pub const DEFAULT_NIX_EVAL_TIMEOUT_SECS: u64 = 600;

pub const CANONICAL_PROVIDER_IDS: &[ProviderId] = &[
    ProviderId::ClaudeCode,
    ProviderId::Codex,
    ProviderId::Omp,
    ProviderId::Antigravity,
    ProviderId::Terminal,
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProviderConfigRecord {
    pub provider_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LspTraceLevel {
    #[default]
    Off,
    Messages,
    Verbose,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FormatterSelection {
    #[default]
    Lsp,
    Biome,
    Nixfmt,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FormattingLanguageSettings {
    pub formatter: FormatterSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FormattingSettings {
    pub javascript_typescript: FormattingLanguageSettings,
    pub json: FormattingLanguageSettings,
    pub nix: FormattingLanguageSettings,
}

impl Default for FormattingSettings {
    fn default() -> Self {
        Self {
            javascript_typescript: FormattingLanguageSettings {
                formatter: FormatterSelection::Biome,
            },
            json: FormattingLanguageSettings {
                formatter: FormatterSelection::Biome,
            },
            nix: FormattingLanguageSettings {
                formatter: FormatterSelection::Nixfmt,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct LspServerConfigRecord {
    pub server_definition_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub runtime_path_override: Option<String>,
    pub runtime_args: Vec<String>,
    pub extra_args: Vec<String>,
    pub trace: LspTraceLevel,
    pub settings: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExternalFolderOpenMode {
    #[default]
    NewWindow,
    FocusedWindow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExternalFileOpenMode {
    #[default]
    PreferFolder,
    FocusedWindow,
    NewWindow,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TabBeamPosition {
    #[default]
    Top,
    Bottom,
}

/// JSON Pointer prefixes of settings that are strictly `GlobalOnly` and cannot be
/// configured in project folder settings.
pub const GLOBAL_ONLY_POINTERS: &[&str] = &["/window"];

pub fn is_global_only_pointer(pointer: &str) -> bool {
    GLOBAL_ONLY_POINTERS.iter().any(|prefix| {
        pointer == *prefix
            || (pointer.starts_with(prefix) && pointer[prefix.len()..].starts_with('/'))
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct WindowSettings {
    /// Theme preference. Current built-in value is system.
    pub theme: String,
    pub external_folder_open_mode: ExternalFolderOpenMode,
    pub external_file_open_mode: ExternalFileOpenMode,
    pub tab_beam_position: TabBeamPosition,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            external_folder_open_mode: ExternalFolderOpenMode::default(),
            external_file_open_mode: ExternalFileOpenMode::default(),
            tab_beam_position: TabBeamPosition::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalSettings {
    pub font_family: String,
    // Upper bound keeps a fat-fingered value a per-field diagnostic instead of
    // a whole-model deserialization failure.
    #[schemars(range(min = 1, max = 65535))]
    pub font_size: u16,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_family: "JetBrains Mono".to_string(),
            font_size: 13,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct NixSettings {
    #[schemars(range(min = 1))]
    pub eval_timeout_secs: u64,
}

impl Default for NixSettings {
    fn default() -> Self {
        Self {
            eval_timeout_secs: DEFAULT_NIX_EVAL_TIMEOUT_SECS,
        }
    }
}

/// VS Code's `files.exclude` defaults (files.contribution.ts). `.git` is never
/// a useful explorer row; the rest are VCS/OS droppings.
pub const DEFAULT_EXPLORER_EXCLUDES: &[&str] = &[
    "**/.git",
    "**/.svn",
    "**/.hg",
    "**/.DS_Store",
    "**/Thumbs.db",
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ExplorerSettings {
    /// Globs hidden from the file explorer, matched against project-relative
    /// paths. Layers merge key by key; map a glob to false to keep it listed.
    pub exclude: BTreeMap<String, bool>,
    /// Hide entries matched by `.gitignore`. VS Code parity: off by default,
    /// so ignored entries are merely dimmed until the user opts in.
    pub exclude_gitignore: bool,
    /// Collapse single-child directory chains into one row ("src/lib").
    pub compact_folders: bool,
}

impl Default for ExplorerSettings {
    fn default() -> Self {
        Self {
            exclude: DEFAULT_EXPLORER_EXCLUDES
                .iter()
                .map(|glob| ((*glob).to_string(), true))
                .collect(),
            exclude_gitignore: false,
            compact_folders: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderSettings {
    pub enabled: bool,
    /// Optional provider executable override. Project settings can change what Sworm executes.
    pub binary_path_override: Option<String>,
    /// Additional provider CLI args. Project settings can change what Sworm executes.
    pub extra_args: Vec<String>,
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            binary_path_override: None,
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LspServerSettings {
    pub enabled: bool,
    /// Optional language server executable override. Project settings can change what Sworm executes.
    pub binary_path_override: Option<String>,
    /// Optional runtime executable override. Project settings can change what Sworm executes.
    pub runtime_path_override: Option<String>,
    /// Additional runtime args. Project settings can change what Sworm executes.
    pub runtime_args: Vec<String>,
    /// Additional language server args. Project settings can change what Sworm executes.
    pub extra_args: Vec<String>,
    pub trace: LspTraceLevel,
    /// Native LSP settings object/value sent to the language server.
    pub settings: Option<Value>,
}

impl Default for LspServerSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            binary_path_override: None,
            runtime_path_override: None,
            runtime_args: Vec::new(),
            extra_args: Vec::new(),
            trace: LspTraceLevel::Off,
            settings: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LspSettings {
    /// Keyed by `BuiltinCatalogService` server_definition_id, formatted as
    /// `${builtin_id}::${server_id}`.
    pub servers: BTreeMap<String, LspServerSettings>,
}

/// # Sworm settings
///
/// Project settings can override executable paths and args for providers and LSP servers. Only trust settings from repositories you trust.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct EffectiveSettings {
    pub window: WindowSettings,
    pub terminal: TerminalSettings,
    pub nix: NixSettings,
    pub explorer: ExplorerSettings,
    pub formatting: FormattingSettings,
    /// Keyed by internal provider ID.
    pub providers: BTreeMap<String, ProviderSettings>,
    pub lsp: LspSettings,
}

impl Default for EffectiveSettings {
    fn default() -> Self {
        Self {
            window: WindowSettings::default(),
            terminal: TerminalSettings::default(),
            nix: NixSettings::default(),
            explorer: ExplorerSettings::default(),
            formatting: FormattingSettings::default(),
            providers: default_provider_settings(),
            lsp: LspSettings::default(),
        }
    }
}

impl EffectiveSettings {
    /// Builds default effective settings with LSP server IDs supplied by
    /// `BuiltinCatalogService::list_server_definitions()`.
    pub fn with_lsp_server_ids<I, S>(server_definition_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut settings = Self::default();
        settings.lsp.servers = server_definition_ids
            .into_iter()
            .map(|id| (id.as_ref().to_string(), LspServerSettings::default()))
            .collect();
        settings
    }
}

/// Runtime + editor JSON Schema for a settings layer file. Derived from the
/// model; only the map key sets (provider ids, LSP server ids) are injected
/// because they come from runtime catalogs, not types.
///
/// When generated for `SettingsLayerKind::Folder`, settings with global-only scope
/// (defined in `GLOBAL_ONLY_POINTERS`) are pruned from the schema so that editors omit
/// them from autocomplete and layer validation rejects them.
pub fn settings_layer_schema(layer: SettingsLayerKind, lsp_server_ids: &[String]) -> Value {
    let mut schema =
        serde_json::to_value(schema_for!(EffectiveSettings)).expect("settings schema serializes");
    restrict_map_keys(
        &mut schema,
        &["providers"],
        CANONICAL_PROVIDER_IDS
            .iter()
            .map(|provider_id| provider_id.to_string())
            .collect(),
    );
    restrict_map_keys(&mut schema, &["lsp", "servers"], lsp_server_ids.to_vec());

    if layer == SettingsLayerKind::Folder {
        for pointer in GLOBAL_ONLY_POINTERS {
            let segments: Vec<&str> = pointer.trim_start_matches('/').split('/').collect();
            if segments.len() == 1 {
                if let Some(props) = schema
                    .pointer_mut("/properties")
                    .and_then(Value::as_object_mut)
                {
                    props.remove(segments[0]);
                }
            } else {
                let field_path = &segments[..segments.len() - 1];
                let prop_name = segments.last().unwrap();
                let p = resolve_schema_pointer(&schema, field_path);
                if let Some(props) = schema
                    .pointer_mut(&format!("{p}/properties"))
                    .and_then(Value::as_object_mut)
                {
                    props.remove(*prop_name);
                }
            }
        }
    }

    schema
}

/// Pins the accepted key set of a map-valued setting. Struct-typed fields are
/// `$ref`s into `definitions` (wrapped in `allOf` when schemars attaches field
/// metadata), so each step dereferences before descending.
fn restrict_map_keys(schema: &mut Value, field_path: &[&str], keys: Vec<String>) {
    let pointer = resolve_schema_pointer(schema, field_path);
    schema
        .pointer_mut(&pointer)
        .and_then(Value::as_object_mut)
        .unwrap_or_else(|| panic!("settings schema exposes {pointer}"))
        .insert("propertyNames".to_string(), json!({ "enum": keys }));
}

fn resolve_schema_pointer(schema: &Value, field_path: &[&str]) -> String {
    let mut pointer = String::new();
    for field in field_path {
        while let Some(reference) = schema.pointer(&pointer).and_then(field_ref) {
            pointer = reference.trim_start_matches('#').to_string();
        }
        pointer.push_str("/properties/");
        pointer.push_str(field);
    }
    while let Some(reference) = schema.pointer(&pointer).and_then(field_ref) {
        pointer = reference.trim_start_matches('#').to_string();
    }
    pointer
}

fn field_ref(node: &Value) -> Option<&str> {
    node.get("$ref")
        .or_else(|| node.pointer("/allOf/0/$ref"))?
        .as_str()
}

fn default_provider_settings() -> BTreeMap<String, ProviderSettings> {
    CANONICAL_PROVIDER_IDS
        .iter()
        .map(|provider_id| (provider_id.to_string(), ProviderSettings::default()))
        .collect()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsLayerKind {
    Global,
    Folder,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SettingsDiagnosticCode {
    /// The layer file could not be read or parsed.
    ParseError,
    /// A value was rejected: wrong type, out of range, bad enum, or null.
    InvalidValue,
    /// An unknown property, provider id, or LSP server id.
    UnknownKey,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsDiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SettingsDiagnostic {
    pub layer: SettingsLayerKind,
    pub path: String,
    pub pointer: String,
    pub code: SettingsDiagnosticCode,
    pub severity: SettingsDiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct SettingsChangedEvent {
    pub layer: SettingsLayerKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    pub generation: u64,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_defaults_include_all_provider_ids() {
        let settings = EffectiveSettings::default();
        let provider_ids: Vec<_> = settings.providers.keys().cloned().collect();

        assert_eq!(
            provider_ids,
            vec!["antigravity", "claude_code", "codex", "omp", "terminal"]
        );
        assert_eq!(
            settings.providers["claude_code"],
            ProviderSettings::default()
        );
    }

    #[test]
    fn lsp_defaults_are_keyed_by_builtin_server_definition_ids() {
        let settings = EffectiveSettings::with_lsp_server_ids(["dev.sworm.vtsls::vtsls"]);

        assert_eq!(
            settings.lsp.servers["dev.sworm.vtsls::vtsls"],
            LspServerSettings::default()
        );
    }

    #[test]
    fn global_schema_includes_window_settings() {
        let schema = settings_layer_schema(SettingsLayerKind::Global, &[]);
        assert!(schema.pointer("/properties/window").is_some());
    }

    #[test]
    fn folder_schema_prunes_global_only_settings() {
        let schema = settings_layer_schema(SettingsLayerKind::Folder, &[]);
        assert!(schema.pointer("/properties/window").is_none());
        assert!(schema.pointer("/properties/terminal").is_some());
        assert!(schema.pointer("/properties/nix").is_some());
    }
}
