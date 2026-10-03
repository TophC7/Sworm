//! Comment CRUD: add, list, update, delete.

use super::queries::{append_event, ensure_issue_exists, list_comments_conn, next_id};
use super::rows::row_to_comment;
use super::validators::{actor, validate_non_empty, DEFAULT_ACTOR};
use super::{db_error, IssueService};
use crate::errors::ApiError;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::json;
use std::path::Path;
use sworm_protocol::issues::*;

impl IssueService {
    /// Append a comment to an issue and return the inserted row.
    pub fn add_comment(
        &self,
        project_path: &Path,
        input: IssueCommentCreateInput,
    ) -> Result<IssueComment, ApiError> {
        validate_non_empty(&input.body)?;
        validate_non_empty(&input.author)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start comment create tx"))?;
        ensure_issue_exists(&tx, &input.issue_id)?;
        let id = next_id(&tx, "comment")?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "INSERT INTO issue_comments(id, issue_id, author, body, created_by, updated_by, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?6)",
            params![id, input.issue_id, input.author, input.body, actor, now],
        ).map_err(db_error("Failed to create comment"))?;
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
            .map_err(db_error("Failed to commit comment create"))?;
        Ok(IssueComment {
            id,
            issue_id: input.issue_id,
            author: input.author,
            body: input.body,
            created_by: actor.to_string(),
            updated_by: actor.to_string(),
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// All comments on an issue, ordered created-at ascending.
    pub fn list_comments(
        &self,
        project_path: &Path,
        issue_id: &str,
    ) -> Result<Vec<IssueComment>, ApiError> {
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
    ) -> Result<IssueComment, ApiError> {
        validate_non_empty(&input.body)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start comment update tx"))?;
        let existing = tx
            .query_row(
                "SELECT id, issue_id, author, body, created_by, updated_by, created_at, updated_at FROM issue_comments WHERE id = ?1",
                params![comment_id],
                row_to_comment,
            )
            .optional()
            .map_err(db_error("Failed to load comment"))?
            .ok_or_else(|| ApiError::NotFound(format!("Comment not found: {}", comment_id)))?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE issue_comments SET body = ?1, updated_by = ?2, updated_at = ?3 WHERE id = ?4",
            params![input.body, actor, now, comment_id],
        )
        .map_err(db_error("Failed to update comment"))?;
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
            .map_err(db_error("Failed to commit comment update"))?;
        Ok(IssueComment {
            body: input.body,
            updated_by: actor.to_string(),
            updated_at: now,
            ..existing
        })
    }

    /// Hard-delete a comment.
    pub fn delete_comment(&self, project_path: &Path, comment_id: &str) -> Result<(), ApiError> {
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(db_error("Failed to start comment delete tx"))?;
        let exists: Option<String> = tx
            .query_row(
                "SELECT id FROM issue_comments WHERE id = ?1",
                params![comment_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error("Failed to load comment"))?;
        if exists.is_none() {
            return Err(ApiError::NotFound(format!(
                "Comment not found: {}",
                comment_id
            )));
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
        .map_err(db_error("Failed to delete comment"))?;
        tx.commit()
            .map_err(db_error("Failed to commit comment delete"))
    }
}
