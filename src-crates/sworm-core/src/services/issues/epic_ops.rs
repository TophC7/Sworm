//! Epic CRUD: create, list, get, update, delete.

use super::queries::{append_event, ensure_epic_exists, get_epic_conn, next_id};
use super::rows::{collect_rows, row_to_epic, EPIC_COLUMNS};
use super::validators::{
    actor, validate_epic_status, validate_non_empty, validate_priority, DEFAULT_ACTOR,
};
use super::{db_error, IssueService};
use crate::errors::ApiError;
use chrono::Utc;
use rusqlite::params;
use serde_json::json;
use std::path::Path;
use sworm_protocol::issues::*;

impl IssueService {
    /// Create an epic. Allocates the next id under the project's
    /// epic-prefix in the same transaction as the insert.
    pub fn create_epic(
        &self,
        project_path: &Path,
        input: IssueEpicCreateInput,
    ) -> Result<IssueEpic, ApiError> {
        validate_non_empty(&input.title)?;
        let status = input.status.unwrap_or_else(|| "todo".to_string());
        validate_epic_status(&status)?;
        let priority = input.priority.unwrap_or(2);
        validate_priority(priority)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start epic create tx"))?;
        let id = next_id(&tx, "epic")?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO issue_epics(id, title, description, status, priority, created_by, updated_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7, ?7)",
            params![id, input.title, input.description, status, priority, actor, now],
        )
        .map_err(db_error("Failed to create epic"))?;
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
            .map_err(db_error("Failed to commit epic create"))?;
        Ok(IssueEpic {
            id,
            title: input.title,
            description: input.description,
            status,
            priority,
            created_by: actor.to_string(),
            updated_by: actor.to_string(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// All epics, ordered priority ascending then created-at ascending.
    pub fn list_epics(&self, project_path: &Path) -> Result<Vec<IssueEpic>, ApiError> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let mut stmt = conn.prepare(&format!("SELECT {EPIC_COLUMNS} FROM issue_epics e ORDER BY e.priority ASC, e.created_at ASC"))
            .map_err(db_error("Failed to prepare epics query"))?;
        let rows = stmt
            .query_map([], row_to_epic)
            .map_err(db_error("Failed to query epics"))?;
        collect_rows(rows, "epic")
    }

    /// Fetch a single epic by id; returns `NotFound` when missing.
    pub fn get_epic(&self, project_path: &Path, epic_id: &str) -> Result<IssueEpic, ApiError> {
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
    ) -> Result<IssueEpic, ApiError> {
        let actor = actor(patch.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start epic update tx"))?;
        let existing = get_epic_conn(&tx, epic_id)?;
        if let Some(title) = patch.title.as_deref() {
            validate_non_empty(title)?;
        }
        if let Some(status) = patch.status.as_deref() {
            validate_epic_status(status)?;
        }
        if let Some(priority) = patch.priority {
            validate_priority(priority)?;
        }
        let now = Utc::now().to_rfc3339();
        let epic = IssueEpic {
            title: patch.title.unwrap_or(existing.title),
            description: patch.description.or(existing.description),
            status: patch.status.unwrap_or(existing.status),
            priority: patch.priority.unwrap_or(existing.priority),
            updated_by: actor.to_string(),
            updated_at: now,
            ..existing
        };
        tx.execute(
            "UPDATE issue_epics SET title = ?1, description = ?2, status = ?3, priority = ?4, updated_by = ?5, updated_at = ?6 WHERE id = ?7",
            params![epic.title, epic.description, epic.status, epic.priority, epic.updated_by, epic.updated_at, epic_id],
        ).map_err(db_error("Failed to update epic"))?;
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
            .map_err(db_error("Failed to commit epic update"))?;
        Ok(epic)
    }

    /// Delete an epic. Refuses if any issue still references it; the
    /// caller must reassign or delete those issues first.
    pub fn delete_epic(&self, project_path: &Path, epic_id: &str) -> Result<(), ApiError> {
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start epic delete tx"))?;
        ensure_epic_exists(&tx, epic_id)?;
        let issue_count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM issue_items i WHERE i.epic_id = ?1",
                params![epic_id],
                |row| row.get(0),
            )
            .map_err(db_error("Failed to count epic issues"))?;
        if issue_count > 0 {
            return Err(ApiError::InvalidArgument(
                "Cannot delete epic while it has issues".to_string(),
            ));
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
            .map_err(db_error("Failed to delete epic"))?;
        tx.commit()
            .map_err(db_error("Failed to commit epic delete"))
    }
}
