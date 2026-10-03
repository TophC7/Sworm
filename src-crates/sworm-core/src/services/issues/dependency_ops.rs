//! Dependency edge management: add, remove, list, cycle detection.

use super::queries::{
    append_event, ensure_issue_exists, list_dependencies_conn, would_create_cycle,
};
use super::validators::actor;
use super::{db_error, IssueService};
use crate::errors::ApiError;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use std::path::Path;
use sworm_protocol::issues::*;
use uuid::Uuid;

impl IssueService {
    /// Add a dependency edge between two issues. Rejects self-loops,
    /// duplicate edges, and edges that would close a cycle.
    pub fn add_dependency(
        &self,
        project_path: &Path,
        input: IssueDependencyInput,
    ) -> Result<IssueDependency, ApiError> {
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start dependency tx"))?;
        ensure_issue_exists(&tx, &input.issue_id)?;
        ensure_issue_exists(&tx, &input.depends_on_issue_id)?;
        if input.issue_id == input.depends_on_issue_id {
            return Err(ApiError::InvalidArgument(
                "Issue cannot depend on itself".to_string(),
            ));
        }
        let existing: Option<String> = tx.query_row("SELECT id FROM issue_dependencies WHERE issue_id = ?1 AND depends_on_issue_id = ?2", params![input.issue_id, input.depends_on_issue_id], |row| row.get(0))
            .optional().map_err(db_error("Failed to check dependency"))?;
        if existing.is_some() {
            return Err(ApiError::InvalidArgument(format!(
                "Dependency already exists: {} → {}",
                input.issue_id, input.depends_on_issue_id
            )));
        }
        if would_create_cycle(&tx, &input.issue_id, &input.depends_on_issue_id)? {
            return Err(ApiError::InvalidArgument(format!(
                "Adding dependency would create a cycle: {} → {}",
                input.issue_id, input.depends_on_issue_id
            )));
        }
        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        tx.execute("INSERT INTO issue_dependencies(id, issue_id, depends_on_issue_id, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, input.issue_id, input.depends_on_issue_id, actor, now])
            .map_err(db_error("Failed to add dependency"))?;
        append_event(
            &tx,
            actor,
            "create",
            "dependency",
            &id,
            Some(json!({"id": id}).to_string()),
            None,
        )?;
        tx.commit()
            .map_err(db_error("Failed to commit dependency add"))?;
        Ok(IssueDependency {
            id,
            issue_id: input.issue_id,
            depends_on_issue_id: input.depends_on_issue_id,
            created_by: actor.to_string(),
            created_at: now,
        })
    }

    /// Remove a dependency edge. Errors with `not found` when the
    /// caller-supplied pair has no matching row.
    pub fn remove_dependency(
        &self,
        project_path: &Path,
        input: IssueDependencyInput,
    ) -> Result<(), ApiError> {
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start dependency remove tx"))?;
        let dep_id: String = tx.query_row("SELECT id FROM issue_dependencies WHERE issue_id = ?1 AND depends_on_issue_id = ?2", params![input.issue_id, input.depends_on_issue_id], |row| row.get(0))
            .optional().map_err(db_error("Failed to load dependency"))?.ok_or_else(|| ApiError::NotFound(format!("Dependency not found: {} → {}", input.issue_id, input.depends_on_issue_id)))?;
        append_event(
            &tx,
            actor,
            "delete",
            "dependency",
            &dep_id,
            Some(json!({"id": dep_id}).to_string()),
            None,
        )?;
        tx.execute(
            "DELETE FROM issue_dependencies WHERE id = ?1",
            params![dep_id],
        )
        .map_err(db_error("Failed to remove dependency"))?;
        tx.commit()
            .map_err(db_error("Failed to commit dependency remove"))
    }

    /// Outgoing dependency edges from an issue (i.e. issues it
    /// depends on).
    pub fn list_dependencies(
        &self,
        project_path: &Path,
        issue_id: &str,
    ) -> Result<Vec<IssueDependency>, ApiError> {
        let db = self.db(project_path)?;
        let conn = db.read();
        ensure_issue_exists(&conn, issue_id)?;
        list_dependencies_conn(&conn, issue_id, false)
    }
}
