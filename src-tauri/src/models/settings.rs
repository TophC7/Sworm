use crate::models::provider::ProviderId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
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

fn default_nix_eval_timeout_secs() -> u64 {
    DEFAULT_NIX_EVAL_TIMEOUT_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct ProviderConfigRecord {
    pub provider_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LspTraceLevel {
    Off,
    Messages,
    Verbose,
}

impl Default for LspTraceLevel {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FormatterSelection {
    Lsp,
    Biome,
    Nixfmt,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct FormattingLanguageSettings {
    pub formatter: FormatterSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct GeneralSettings {
    pub theme: String,
    pub terminal_font_family: String,
    pub terminal_font_size: u16,
    #[serde(default = "default_nix_eval_timeout_secs")]
    pub nix_eval_timeout_secs: u64,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            terminal_font_family: "JetBrains Mono".to_string(),
            terminal_font_size: 13,
            nix_eval_timeout_secs: DEFAULT_NIX_EVAL_TIMEOUT_SECS,
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
pub struct ExplorerSettings {
    /// Glob -> enabled. Matched against project-relative paths.
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
pub struct ProviderSettings {
    pub enabled: bool,
    pub binary_path_override: Option<String>,
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
pub struct LspServerSettings {
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub runtime_path_override: Option<String>,
    pub runtime_args: Vec<String>,
    pub extra_args: Vec<String>,
    pub trace: LspTraceLevel,
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
pub struct LspSettings {
    pub servers: BTreeMap<String, LspServerSettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EffectiveSettings {
    pub general: GeneralSettings,
    pub explorer: ExplorerSettings,
    pub formatting: FormattingSettings,
    pub providers: BTreeMap<String, ProviderSettings>,
    pub lsp: LspSettings,
}

impl Default for EffectiveSettings {
    fn default() -> Self {
        Self {
            general: GeneralSettings::default(),
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsDiagnosticCode {
    ParseError,
    TypeError,
    InvalidEnum,
    InvalidNull,
    UnknownKey,
    UnknownProvider,
    UnknownLspServer,
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
    use schemars::schema_for;

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
    fn canonical_model_has_schema_for_known_sections() {
        let schema = schema_for!(EffectiveSettings);
        let schema_value = serde_json::to_value(schema).expect("schema serializes");

        assert!(schema_value.to_string().contains("general"));
        assert!(schema_value.to_string().contains("formatting"));
        assert!(schema_value.to_string().contains("providers"));
        assert!(schema_value.to_string().contains("lsp"));
    }
}
