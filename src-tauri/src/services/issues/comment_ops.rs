//! Comment CRUD: add, list, update, delete.

use super::queries::{append_event, ensure_issue_exists, list_comments_conn, next_id};
use super::validators::{actor, validate_title, DEFAULT_ACTOR};
use super::IssueService;
use crate::models::issues::*;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use std::path::Path;

impl IssueService {
    /// Append a comment to an issue and return the inserted row.
    pub fn add_comment(
        &self,
        project_path: &Path,
        input: IssueCommentCreateInput,
    ) -> Result<IssueComment, String> {
        validate_title(&input.body)?;
        validate_title(&input.author)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start comment create tx: {}", e))?;
        ensure_issue_exists(&tx, &input.issue_id)?;
        let id = next_id(&tx, "comment", "comment_prefix")?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO issue_comments(id, issue_id, author, body, created_by, updated_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?6)",
            params![id, input.issue_id, input.author, input.body, actor, now],
        ).map_err(|e| format!("Failed to create comment: {}", e))?;
        append_event(
            &tx,
            actor,
            "create",
            "comment",
            &id,
            Some(json!({"id": id}).to_string()),
            None,
        )?;
        tx.commit()
            .map_err(|e| format!("Failed to commit comment create: {}", e))?;
        drop(conn);
        self.list_comments(project_path, &input.issue_id)?
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| format!("Comment missing after create: {}", id))
    }

    /// All comments on an issue, ordered created-at ascending.
    pub fn list_comments(
        &self,
        project_path: &Path,
        issue_id: &str,
    ) -> Result<Vec<IssueComment>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        ensure_issue_exists(&conn, issue_id)?;
        list_comments_conn(&conn, issue_id)
    }

    /// Replace a comment's body. Audit log records an `update` event.
    pub fn update_comment(
        &self,
        project_path: &Path,
        comment_id: &str,
        input: IssueCommentUpdateInput,
    ) -> Result<IssueComment, String> {
        validate_title(&input.body)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start comment update tx: {}", e))?;
        let issue_id: String = tx
            .query_row(
                "SELECT issue_id FROM issue_comments WHERE id = ?1",
                params![comment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to load comment: {}", e))?
            .ok_or_else(|| format!("Comment not found: {}", comment_id))?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE issue_comments SET body = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
            params![input.body, actor, now, comment_id],
        )
        .map_err(|e| format!("Failed to update comment: {}", e))?;
        append_event(
            &tx,
            actor,
            "update",
            "comment",
            comment_id,
            None,
            Some(json!({"updated": true}).to_string()),
        )?;
        tx.commit()
            .map_err(|e| format!("Failed to commit comment update: {}", e))?;
        drop(conn);
        self.list_comments(project_path, &issue_id)?
            .into_iter()
            .find(|c| c.id == comment_id)
            .ok_or_else(|| format!("Comment missing after update: {}", comment_id))
    }

    /// Hard-delete a comment.
    pub fn delete_comment(&self, project_path: &Path, comment_id: &str) -> Result<(), String> {
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start comment delete tx: {}", e))?;
        let exists: Option<String> = tx
            .query_row(
                "SELECT id FROM issue_comments WHERE id = ?1",
                params![comment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("Failed to load comment: {}", e))?;
        if exists.is_none() {
            return Err(format!("Comment not found: {}", comment_id));
        }
        append_event(
            &tx,
            DEFAULT_ACTOR,
            "delete",
            "comment",
            comment_id,
            Some(json!({"id": comment_id}).to_string()),
            None,
        )?;
        tx.execute(
            "DELETE FROM issue_comments WHERE id = ?1",
            params![comment_id],
        )
        .map_err(|e| format!("Failed to delete comment: {}", e))?;
        tx.commit()
            .map_err(|e| format!("Failed to commit comment delete: {}", e))
    }
}
