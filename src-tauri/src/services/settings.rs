use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfigRecord {
    pub provider_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub extra_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LspTraceLevel {
    Off,
    Messages,
    Verbose,
}

impl LspTraceLevel {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Messages => "messages",
            Self::Verbose => "verbose",
        }
    }

    fn from_db_str(value: &str) -> Self {
        match value {
            "messages" => Self::Messages,
            "verbose" => Self::Verbose,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FormatterSelection {
    #[default]
    Auto,
    Lsp,
    Biome,
    Nixfmt,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FormattingLanguageSettings {
    #[serde(default)]
    pub formatter: FormatterSelection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormattingSettings {
    #[serde(default)]
    pub javascript_typescript: FormattingLanguageSettings,
    #[serde(default)]
    pub json: FormattingLanguageSettings,
    #[serde(default)]
    pub nix: FormattingLanguageSettings,
}

impl Default for FormattingSettings {
    fn default() -> Self {
        Self {
            javascript_typescript: FormattingLanguageSettings::default(),
            json: FormattingLanguageSettings::default(),
            nix: FormattingLanguageSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfigRecord {
    pub server_definition_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub runtime_path_override: Option<String>,
    pub runtime_args: Vec<String>,
    pub extra_args: Vec<String>,
    pub trace: LspTraceLevel,
    pub settings_json: Option<String>,
}

impl LspServerConfigRecord {
    pub fn default_for(server_definition_id: &str) -> Self {
        Self {
            server_definition_id: server_definition_id.to_string(),
            enabled: true,
            binary_path_override: None,
            runtime_path_override: None,
            runtime_args: Vec::new(),
            extra_args: Vec::new(),
            trace: LspTraceLevel::Off,
            settings_json: None,
        }
    }
}

impl ProviderConfigRecord {
    pub fn default_for(provider_id: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            enabled: true,
            binary_path_override: None,
            extra_args: Vec::new(),
        }
    }
}

/// Default timeout for `nix develop --command env -0` evaluations, in seconds.
/// 120s was too aggressive for cold stores pulling GUI deps (webkitgtk, gtk3,
/// rust toolchain); 600s leaves headroom while still catching true hangs.
pub const DEFAULT_NIX_EVAL_TIMEOUT_SECS: u64 = 600;

fn default_nix_eval_timeout_secs() -> u64 {
    DEFAULT_NIX_EVAL_TIMEOUT_SECS
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub theme: String,
    pub terminal_font_family: String,
    pub terminal_font_size: u16,
    // Backward-compat: older stored JSON won't have this key -- fall back to default.
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

pub struct SettingsService;

impl SettingsService {
    pub fn get(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
        conn.query_row(
            "SELECT value_json FROM app_settings WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
    }

    pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO app_settings (key, value_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value_json = ?2, updated_at = ?3",
            rusqlite::params![key, value, now],
        )?;
        Ok(())
    }

    pub fn get_general_settings(conn: &Connection) -> Result<GeneralSettings, String> {
        match Self::get(conn, "general").map_err(|error| error.to_string())? {
            Some(value) => serde_json::from_str(&value)
                .map_err(|error| format!("Failed to parse general settings: {}", error)),
            None => Ok(GeneralSettings::default()),
        }
    }

    pub fn set_general_settings(
        conn: &Connection,
        settings: &GeneralSettings,
    ) -> Result<(), String> {
        let serialized = serde_json::to_string(settings)
            .map_err(|error| format!("Failed to serialize general settings: {}", error))?;
        Self::set(conn, "general", &serialized).map_err(|error| error.to_string())
    }

    pub fn get_formatting_settings(conn: &Connection) -> Result<FormattingSettings, String> {
        match Self::get(conn, "formatting").map_err(|error| error.to_string())? {
            Some(value) => serde_json::from_str(&value)
                .map_err(|error| format!("Failed to parse formatting settings: {}", error)),
            None => Ok(FormattingSettings::default()),
        }
    }

    pub fn set_formatting_settings(
        conn: &Connection,
        settings: &FormattingSettings,
    ) -> Result<(), String> {
        let serialized = serde_json::to_string(settings)
            .map_err(|error| format!("Failed to serialize formatting settings: {}", error))?;
        Self::set(conn, "formatting", &serialized).map_err(|error| error.to_string())
    }

    pub fn get_provider_config(
        conn: &Connection,
        provider_id: &str,
    ) -> Result<Option<ProviderConfigRecord>, String> {
        conn.query_row(
            "SELECT provider_id, enabled, binary_path_override, extra_args_json
             FROM provider_configs WHERE provider_id = ?1",
            [provider_id],
            |row| {
                let extra_args_json: Option<String> = row.get(3)?;
                let extra_args = extra_args_json
                    .as_deref()
                    .map(serde_json::from_str::<Vec<String>>)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            3,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?
                    .unwrap_or_default();

                Ok(ProviderConfigRecord {
                    provider_id: row.get(0)?,
                    enabled: row.get::<_, i64>(1)? != 0,
                    binary_path_override: row.get(2)?,
                    extra_args,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed to load provider config: {}", error))
    }

    pub fn save_provider_config(
        conn: &Connection,
        config: &ProviderConfigRecord,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let extra_args_json = serde_json::to_string(&config.extra_args)
            .map_err(|error| format!("Failed to serialize provider args: {}", error))?;

        conn.execute(
            "INSERT INTO provider_configs (
                provider_id, enabled, binary_path_override, extra_args_json, env_overrides_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, '{}', ?5)
             ON CONFLICT(provider_id) DO UPDATE SET
                enabled = ?2,
                binary_path_override = ?3,
                extra_args_json = ?4,
                updated_at = ?5",
            rusqlite::params![
                config.provider_id,
                if config.enabled { 1 } else { 0 },
                config.binary_path_override,
                extra_args_json,
                now
            ],
        )
        .map_err(|error| format!("Failed to save provider config: {}", error))?;

        Ok(())
    }

    pub fn load_binary_overrides(conn: &Connection) -> Result<HashMap<String, String>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT provider_id, binary_path_override
                 FROM provider_configs
                 WHERE binary_path_override IS NOT NULL
                   AND TRIM(binary_path_override) != ''",
            )
            .map_err(|error| format!("Failed to prepare provider overrides query: {}", error))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|error| format!("Failed to query provider overrides: {}", error))?;

        let mut overrides = HashMap::new();
        for row in rows {
            let (provider_id, binary_path_override) =
                row.map_err(|error| format!("Failed to read provider override: {}", error))?;
            overrides.insert(provider_id, binary_path_override);
        }

        Ok(overrides)
    }

    pub fn get_lsp_server_config(
        conn: &Connection,
        server_definition_id: &str,
    ) -> Result<Option<LspServerConfigRecord>, String> {
        conn.query_row(
            "SELECT
                server_definition_id,
                enabled,
                binary_path_override,
                runtime_path_override,
                runtime_args_json,
                extra_args_json,
                trace,
                settings_json
             FROM lsp_server_configs
             WHERE server_definition_id = ?1",
            [server_definition_id],
            |row| {
                let runtime_args_json: Option<String> = row.get(4)?;
                let extra_args_json: Option<String> = row.get(5)?;

                let runtime_args = runtime_args_json
                    .as_deref()
                    .map(serde_json::from_str::<Vec<String>>)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?
                    .unwrap_or_default();

                let extra_args = extra_args_json
                    .as_deref()
                    .map(serde_json::from_str::<Vec<String>>)
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?
                    .unwrap_or_default();

                let trace: String = row.get(6)?;

                Ok(LspServerConfigRecord {
                    server_definition_id: row.get(0)?,
                    enabled: row.get::<_, i64>(1)? != 0,
                    binary_path_override: row.get(2)?,
                    runtime_path_override: row.get(3)?,
                    runtime_args,
                    extra_args,
                    trace: LspTraceLevel::from_db_str(&trace),
                    settings_json: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("Failed to load LSP server config: {}", error))
    }

    pub fn save_lsp_server_config(
        conn: &Connection,
        config: &LspServerConfigRecord,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let runtime_args_json = serde_json::to_string(&config.runtime_args)
            .map_err(|error| format!("Failed to serialize runtime args: {}", error))?;
        let extra_args_json = serde_json::to_string(&config.extra_args)
            .map_err(|error| format!("Failed to serialize LSP args: {}", error))?;

        conn.execute(
            "INSERT INTO lsp_server_configs (
                server_definition_id,
                enabled,
                binary_path_override,
                runtime_path_override,
                runtime_args_json,
                extra_args_json,
                trace,
                settings_json,
                updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(server_definition_id) DO UPDATE SET
                enabled = excluded.enabled,
                binary_path_override = excluded.binary_path_override,
                runtime_path_override = excluded.runtime_path_override,
                runtime_args_json = excluded.runtime_args_json,
                extra_args_json = excluded.extra_args_json,
                trace = excluded.trace,
                settings_json = excluded.settings_json,
                updated_at = excluded.updated_at",
            rusqlite::params![
                config.server_definition_id,
                if config.enabled { 1 } else { 0 },
                config.binary_path_override,
                config.runtime_path_override,
                runtime_args_json,
                extra_args_json,
                config.trace.as_db_str(),
                config.settings_json,
                now
            ],
        )
        .map_err(|error| format!("Failed to save LSP server config: {}", error))?;

        Ok(())
    }
}
