use crate::provider::{ProviderId, ProviderStatus};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LspServerConfigRecord {
    pub server_definition_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
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

/// One paired `sworm-server`, keyed by the `<server>` segment of `sworm://<server>/…`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct RemoteSettings {
    /// `host:port` the daemon listens on.
    pub address: String,
    /// Pinned server certificate fingerprint, `SHA256:<64 lowercase hex>`.
    pub fingerprint: String,
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
    pub remotes: BTreeMap<String, RemoteSettings>,
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
            remotes: BTreeMap::new(),
        }
    }
}

impl EffectiveSettings {
    /// Builds default effective settings with LSP server IDs supplied by
    /// `BuiltinCatalogService::server_definition_ids()`.
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsLayerKind {
    Global,
    Folder,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SettingsDiagnosticCode {
    /// The layer file could not be read or parsed.
    ParseError,
    /// A value was rejected: wrong type, out of range, bad enum, or null.
    InvalidValue,
    /// An unknown property, provider id, or LSP server id.
    UnknownKey,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsDiagnosticSeverity {
    Warning,
    Error,
}

/// Which machine resolved the layer a diagnostic came from. A remote workspace
/// shows both at once: the daemon's layers plus this desktop's.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SettingsOrigin {
    /// Resolved by the process that produced the payload. For a remote
    /// workspace the desktop router retags the daemon's own as `Host`.
    Desktop,
    /// Resolved on the paired daemon that hosts the folder.
    Host,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsDiagnostic {
    pub layer: SettingsLayerKind,
    pub origin: SettingsOrigin,
    pub path: String,
    pub pointer: String,
    pub code: SettingsDiagnosticCode,
    pub severity: SettingsDiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SettingsChangedEvent {
    pub layer: SettingsLayerKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
    pub generation: u64,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettingsEntry {
    pub provider: ProviderStatus,
    pub config: ProviderConfigRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsPayload {
    pub window: WindowSettings,
    pub terminal: TerminalSettings,
    pub nix: NixSettings,
    pub formatting: FormattingSettings,
    pub lsp: LspSettings,
    pub providers: Vec<ProviderSettingsEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveSettingsPayload {
    pub settings: EffectiveSettings,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsLayerPayload {
    pub path: String,
    pub loaded: bool,
    pub value: Value,
    pub diagnostics: Vec<SettingsDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutsFilePayload {
    pub path: String,
    pub loaded: bool,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsFileResult {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchSettingsSectionInput {
    pub section: String,
    pub value: Value,
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
    fn settings_payload_round_trips_global_lsp_defaults_and_overrides() {
        let mut settings = EffectiveSettings::with_lsp_server_ids([
            "dev.sworm.vtsls::vtsls",
            "dev.sworm.rust-analyzer::rust-analyzer",
        ]);
        settings
            .lsp
            .servers
            .get_mut("dev.sworm.vtsls::vtsls")
            .unwrap()
            .extra_args = vec!["--stdio".to_string()];
        let payload = SettingsPayload {
            window: settings.window,
            terminal: settings.terminal,
            nix: settings.nix,
            formatting: settings.formatting,
            lsp: settings.lsp,
            providers: Vec::new(),
        };

        let value = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            value["lsp"]["servers"]["dev.sworm.rust-analyzer::rust-analyzer"],
            serde_json::to_value(LspServerSettings::default()).unwrap()
        );
        let restored: SettingsPayload = serde_json::from_value(value).unwrap();
        assert_eq!(restored.lsp, payload.lsp);
    }
}
