//! Epic CRUD: create, list, get, update, delete.

use super::queries::{append_event, ensure_epic_exists, get_epic_conn, next_id};
use super::rows::{collect_rows, row_to_epic};
use super::validators::{
    actor, validate_epic_status, validate_priority, validate_title, DEFAULT_ACTOR,
};
use super::IssueService;
use crate::models::issues::*;
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::path::Path;

impl IssueService {
    /// Create an epic. Allocates the next id under the project's
    /// epic-prefix in the same transaction as the insert.
    pub fn create_epic(
        &self,
        project_path: &Path,
        input: IssueEpicCreateInput,
    ) -> Result<IssueEpic, String> {
        validate_title(&input.title)?;
        let status = input.status.unwrap_or_else(|| "todo".to_string());
        validate_epic_status(&status)?;
        let priority = input.priority.unwrap_or(2);
        validate_priority(priority)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start epic create tx: {}", e))?;
        let id = next_id(&tx, "epic", "epic_prefix")?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO issue_epics(id, title, description, status, priority, created_by, updated_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?7)",
            params![id, input.title, input.description, status, priority, actor, now],
        )
        .map_err(|e| format!("Failed to create epic: {}", e))?;
        append_event(
            &tx,
            actor,
            "create",
            "epic",
            &id,
            Some(json!({"id": id}).to_string()),
            None,
        )?;
        tx.commit()
            .map_err(|e| format!("Failed to commit epic create: {}", e))?;
        drop(conn);
        self.get_epic(project_path, &id)?
            .ok_or_else(|| format!("Epic missing after create: {}", id))
    }

    /// All epics, ordered priority ascending then created-at ascending.
    pub fn list_epics(&self, project_path: &Path) -> Result<Vec<IssueEpic>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let mut stmt = conn.prepare("SELECT id, title, description, status, priority, created_by, updated_by, created_at, updated_at FROM issue_epics ORDER BY priority ASC, created_at ASC")
            .map_err(|e| format!("Failed to prepare epics query: {}", e))?;
        let rows = stmt
            .query_map([], row_to_epic)
            .map_err(|e| format!("Failed to query epics: {}", e))?;
        collect_rows(rows, "epic")
    }

    /// Fetch a single epic by id; returns `Ok(None)` when missing.
    pub fn get_epic(
        &self,
        project_path: &Path,
        epic_id: &str,
    ) -> Result<Option<IssueEpic>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        get_epic_conn(&conn, epic_id)
    }

    /// Apply a partial update to an epic.
    pub fn update_epic(
        &self,
        project_path: &Path,
        epic_id: &str,
        patch: IssueEpicUpdateInput,
    ) -> Result<IssueEpic, String> {
        let actor = actor(patch.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start epic update tx: {}", e))?;
        let existing =
            get_epic_conn(&tx, epic_id)?.ok_or_else(|| format!("Epic not found: {}", epic_id))?;
        if let Some(title) = patch.title.as_deref() {
            validate_title(title)?;
        }
        if let Some(status) = patch.status.as_deref() {
            validate_epic_status(status)?;
        }
        if let Some(priority) = patch.priority {
            validate_priority(priority)?;
        }
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE issue_epics SET title = ?1, description = ?2, status = ?3, priority = ?4, updated_by = ?5, updated_at = ?6 WHERE id = ?7",
            params![patch.title.unwrap_or(existing.title), patch.description.or(existing.description), patch.status.unwrap_or(existing.status), patch.priority.unwrap_or(existing.priority), actor, now, epic_id],
        ).map_err(|e| format!("Failed to update epic: {}", e))?;
        append_event(
            &tx,
            actor,
            "update",
            "epic",
            epic_id,
            None,
            Some(json!({"updated": true}).to_string()),
        )?;
        tx.commit()
            .map_err(|e| format!("Failed to commit epic update: {}", e))?;
        drop(conn);
        self.get_epic(project_path, epic_id)?
            .ok_or_else(|| format!("Epic missing after update: {}", epic_id))
    }

    /// Delete an epic. Refuses if any issue still references it; the
    /// caller must reassign or delete those issues first.
    pub fn delete_epic(&self, project_path: &Path, epic_id: &str) -> Result<(), String> {
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start epic delete tx: {}", e))?;
        ensure_epic_exists(&tx, epic_id)?;
        let issue_count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM issue_items WHERE epic_id = ?1",
                params![epic_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("Failed to count epic issues: {}", e))?;
        if issue_count > 0 {
            return Err("Cannot delete epic while it has issues".to_string());
        }
        append_event(
            &tx,
            DEFAULT_ACTOR,
            "delete",
            "epic",
            epic_id,
            Some(json!({"id": epic_id}).to_string()),
            None,
        )?;
        tx.execute("DELETE FROM issue_epics WHERE id = ?1", params![epic_id])
            .map_err(|e| format!("Failed to delete epic: {}", e))?;
        tx.commit()
            .map_err(|e| format!("Failed to commit epic delete: {}", e))
    }
}
