// Small key/value store for frontend-owned hot-restore state (workbench
// blob, recent folders). The frontend owns the shape of each value; the
// backend only stores opaque JSON strings.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

pub struct AppStateKvService;

impl AppStateKvService {
    pub fn new() -> Self {
        Self
    }

    pub fn get(&self, conn: &Connection, key: &str) -> Result<Option<String>, String> {
        conn.query_row(
            "SELECT value_json FROM app_state WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("app_state get failed: {}", e))
    }

    pub fn put(&self, conn: &Connection, key: &str, value_json: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO app_state (key, value_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
               value_json = excluded.value_json,
               updated_at = excluded.updated_at",
            rusqlite::params![key, value_json, now],
        )
        .map_err(|e| format!("app_state put failed: {}", e))?;
        Ok(())
    }
}
