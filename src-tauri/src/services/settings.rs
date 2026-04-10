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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub theme: String,
    pub terminal_font_family: String,
    pub terminal_font_size: u16,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            terminal_font_family: "JetBrains Mono".to_string(),
            terminal_font_size: 13,
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

    pub fn set_general_settings(conn: &Connection, settings: &GeneralSettings) -> Result<(), String> {
        let serialized = serde_json::to_string(settings)
            .map_err(|error| format!("Failed to serialize general settings: {}", error))?;
        Self::set(conn, "general", &serialized).map_err(|error| error.to_string())
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
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| format!("Failed to query provider overrides: {}", error))?;

        let mut overrides = HashMap::new();
        for row in rows {
            let (provider_id, binary_path_override) =
                row.map_err(|error| format!("Failed to read provider override: {}", error))?;
            overrides.insert(provider_id, binary_path_override);
        }

        Ok(overrides)
    }
}
