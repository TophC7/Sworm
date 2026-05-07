//! Project-local id-prefix config: list, get, set.

use super::rows::collect_rows;
use super::validators::{validate_config_key, validate_prefix};
use super::IssueService;
use crate::models::issues::*;
use rusqlite::{params, OptionalExtension};
use std::path::Path;

impl IssueService {
    /// Project-local id-prefix configuration entries.
    pub fn list_config(&self, project_path: &Path) -> Result<Vec<IssueConfigEntry>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let mut stmt = conn
            .prepare("SELECT key, value FROM issue_config ORDER BY key ASC")
            .map_err(|e| format!("Failed to prepare config query: {}", e))?;
        let rows = stmt
            .query_map([], |row| {
                Ok(IssueConfigEntry {
                    key: row.get(0)?,
                    value: row.get(1)?,
                })
            })
            .map_err(|e| format!("Failed to query config: {}", e))?;
        collect_rows(rows, "config")
    }

    /// Look up a single config entry by key.
    pub fn get_config(
        &self,
        project_path: &Path,
        key: &str,
    ) -> Result<Option<IssueConfigEntry>, String> {
        validate_config_key(key)?;
        let db = self.db(project_path)?;
        let conn = db.read();
        conn.query_row(
            "SELECT key, value FROM issue_config WHERE key = ?1",
            params![key],
            |row| {
                Ok(IssueConfigEntry {
                    key: row.get(0)?,
                    value: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(|e| format!("Failed to get config: {}", e))
    }

    /// Upsert a single config entry. Validates key/value shape; the
    /// returned row reflects the normalized stored value.
    pub fn set_config(
        &self,
        project_path: &Path,
        key: &str,
        value: &str,
    ) -> Result<IssueConfigEntry, String> {
        validate_config_key(key)?;
        let normalized = validate_prefix(value)?;
        let db = self.db(project_path)?;
        let conn = db.write();
        conn.execute("INSERT INTO issue_config(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, normalized])
            .map_err(|e| format!("Failed to set config: {}", e))?;
        Ok(IssueConfigEntry {
            key: key.to_string(),
            value: normalized,
        })
    }
}
