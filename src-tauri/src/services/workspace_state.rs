// Persistence for per-project workspace layout and app-shell key/value state.
//
// Workspace layout: tabs + panes + split mode for a project, stored as a
// versioned JSON blob indexed by project_id. The frontend owns the shape
// of that blob; the backend only stores it.
//
// App state: a small key/value store scoped to hot-restore data (open
// project ids, active project id). Distinct from `app_settings`, which
// stores user preferences.

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension};

pub struct WorkspaceStateService;

impl WorkspaceStateService {
    pub fn new() -> Self {
        Self
    }

    pub fn get(&self, conn: &Connection, project_id: &str) -> Result<Option<String>, String> {
        conn.query_row(
            "SELECT state_json FROM workspace_state WHERE project_id = ?1",
            rusqlite::params![project_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| format!("workspace_state get failed: {}", e))
    }

    pub fn put(&self, conn: &Connection, project_id: &str, state_json: &str) -> Result<(), String> {
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO workspace_state (project_id, state_json, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(project_id) DO UPDATE SET
               state_json = excluded.state_json,
               updated_at = excluded.updated_at",
            rusqlite::params![project_id, state_json, now],
        )
        .map_err(|e| format!("workspace_state put failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn remove(&self, conn: &Connection, project_id: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM workspace_state WHERE project_id = ?1",
            rusqlite::params![project_id],
        )
        .map_err(|e| format!("workspace_state remove failed: {}", e))?;
        Ok(())
    }
}

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
