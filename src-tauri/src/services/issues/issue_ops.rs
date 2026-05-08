//! Issue CRUD: list, ready, search, get, create, update, delete.

use super::queries::{
    append_event, ensure_epic_exists, ensure_issue_exists, escape_like, get_issue,
    list_comments_conn, list_dependencies_conn, list_events_conn, next_id, query_issues,
};
use super::validators::{
    actor, validate_assignee, validate_issue_status, validate_priority, validate_tags,
    validate_title, DEFAULT_ACTOR,
};
use super::{IssueService, DEFAULT_LIMIT};
use crate::models::issues::*;
use chrono::Utc;
use rusqlite::{params, ToSql};
use serde_json::json;
use std::path::Path;

impl IssueService {
    /// List issues filtered by status / epic / archived flag. Default
    /// ordering is priority ascending, created-at ascending.
    pub fn list(
        &self,
        project_path: &Path,
        filters: IssueListFilters,
    ) -> Result<Vec<Issue>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let mut sql = String::from("SELECT id, epic_id, parent_issue_id, title, description, status, priority, assignee_kind, assignee_id, created_by, updated_by, tags_json, context_json, created_at, updated_at FROM issue_items WHERE 1=1");
        let mut owned: Vec<Box<dyn ToSql>> = Vec::new();

        if let Some(status) = filters.status {
            validate_issue_status(&status)?;
            sql.push_str(" AND status = ?");
            owned.push(Box::new(status));
        } else if !filters.include_archived.unwrap_or(false) {
            sql.push_str(" AND status != 'archived'");
        }
        if let Some(epic_id) = filters.epic_id {
            sql.push_str(" AND epic_id = ?");
            owned.push(Box::new(epic_id));
        }
        sql.push_str(" ORDER BY priority ASC, created_at ASC LIMIT ?");
        owned.push(Box::new(
            filters.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 500),
        ));

        let params = owned
            .iter()
            .map(|v| v.as_ref())
            .collect::<Vec<&dyn ToSql>>();
        query_issues(&conn, &sql, &params)
    }

    /// Top-of-queue todo issues with no unresolved dependencies and no
    /// parent. Used by the agent prompt and the sidebar's ready section.
    pub fn ready(
        &self,
        project_path: &Path,
        filters: IssueReadyFilters,
    ) -> Result<Vec<Issue>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let mut sql = String::from(
            "SELECT id, epic_id, parent_issue_id, title, description, status, priority, assignee_kind, assignee_id, created_by, updated_by, tags_json, context_json, created_at, updated_at
             FROM issue_items i
             WHERE i.status = 'todo'
               AND i.parent_issue_id IS NULL
               AND NOT EXISTS (
                 SELECT 1 FROM issue_dependencies d
                 JOIN issue_items dep ON dep.id = d.depends_on_issue_id
                 WHERE d.issue_id = i.id
                   AND dep.status NOT IN ('completed', 'wont_fix', 'archived')
               )",
        );
        let mut owned: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(epic_id) = filters.epic_id {
            sql.push_str(" AND i.epic_id = ?");
            owned.push(Box::new(epic_id));
        }
        sql.push_str(" ORDER BY i.priority ASC, i.created_at ASC LIMIT ?");
        owned.push(Box::new(
            filters.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 500),
        ));
        let params = owned
            .iter()
            .map(|v| v.as_ref())
            .collect::<Vec<&dyn ToSql>>();
        query_issues(&conn, &sql, &params)
    }

    /// Substring match across issue title, description, comment body,
    /// and epic title. The query is escaped with `\` so user wildcards
    /// (`%`, `_`) are treated as literal characters.
    pub fn search(
        &self,
        project_path: &Path,
        query: &str,
        filters: IssueSearchFilters,
    ) -> Result<Vec<Issue>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let like = format!("%{}%", escape_like(query));
        let mut sql = String::from(
            "SELECT DISTINCT i.id, i.epic_id, i.parent_issue_id, i.title, i.description, i.status, i.priority, i.assignee_kind, i.assignee_id, i.created_by, i.updated_by, i.tags_json, i.context_json, i.created_at, i.updated_at
             FROM issue_items i
             LEFT JOIN issue_comments c ON c.issue_id = i.id
             LEFT JOIN issue_epics e ON e.id = i.epic_id
             WHERE (i.title LIKE ? ESCAPE '\\' OR COALESCE(i.description, '') LIKE ? ESCAPE '\\' OR COALESCE(c.body, '') LIKE ? ESCAPE '\\' OR COALESCE(e.title, '') LIKE ? ESCAPE '\\')",
        );
        let mut owned: Vec<Box<dyn ToSql>> = vec![
            Box::new(like.clone()),
            Box::new(like.clone()),
            Box::new(like.clone()),
            Box::new(like),
        ];

        if let Some(status) = filters.status {
            validate_issue_status(&status)?;
            sql.push_str(" AND i.status = ?");
            owned.push(Box::new(status));
        } else if !filters.include_archived.unwrap_or(false) {
            sql.push_str(" AND i.status != 'archived'");
        }
        if let Some(epic_id) = filters.epic_id {
            sql.push_str(" AND i.epic_id = ?");
            owned.push(Box::new(epic_id));
        }
        sql.push_str(" ORDER BY i.priority ASC, i.updated_at DESC LIMIT ?");
        owned.push(Box::new(
            filters.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, 500),
        ));
        let params = owned
            .iter()
            .map(|v| v.as_ref())
            .collect::<Vec<&dyn ToSql>>();
        query_issues(&conn, &sql, &params)
    }

    /// Detail lookup: issue plus comments, dependency edges in both
    /// directions, sub-issues, and the recent event tail.
    pub fn get(&self, project_path: &Path, issue_id: &str) -> Result<Option<IssueDetail>, String> {
        let db = self.db(project_path)?;
        let conn = db.read();
        let Some(issue) = get_issue(&conn, issue_id)? else {
            return Ok(None);
        };
        let comments = list_comments_conn(&conn, issue_id)?;
        let depends_on = list_dependencies_conn(&conn, issue_id, false)?;
        let blocked_by = list_dependencies_conn(&conn, issue_id, true)?;
        let sub_issues = query_issues(
            &conn,
            "SELECT id, epic_id, parent_issue_id, title, description, status, priority, assignee_kind, assignee_id, created_by, updated_by, tags_json, context_json, created_at, updated_at FROM issue_items WHERE parent_issue_id = ? ORDER BY priority ASC, created_at ASC",
            &[&issue_id],
        )?;
        let events = list_events_conn(&conn, issue_id)?;
        Ok(Some(IssueDetail {
            issue,
            comments,
            depends_on,
            blocked_by,
            sub_issues,
            events,
        }))
    }

    /// Insert an issue. Validates inputs, allocates the next id under
    /// the project's prefix, and writes a matching `create` event in
    /// the same transaction.
    pub fn create(&self, project_path: &Path, input: IssueCreateInput) -> Result<Issue, String> {
        validate_title(&input.title)?;
        let status = input.status.unwrap_or_else(|| "todo".to_string());
        validate_issue_status(&status)?;
        let priority = input.priority.unwrap_or(2);
        validate_priority(priority)?;
        let assignee_kind = input
            .assignee_kind
            .unwrap_or_else(|| "unassigned".to_string());
        validate_assignee(&assignee_kind, input.assignee_id.as_deref())?;
        validate_tags(&input.tags)?;
        let actor = actor(input.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start issue create tx: {}", e))?;

        let epic_id = if let Some(parent_id) = input.parent_issue_id.as_deref() {
            ensure_issue_exists(&tx, parent_id)?;
            let (parent_parent, parent_epic): (Option<String>, Option<String>) = tx
                .query_row(
                    "SELECT parent_issue_id, epic_id FROM issue_items WHERE id = ?1",
                    params![parent_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|e| format!("Failed to validate parent issue: {}", e))?;
            if parent_parent.is_some() {
                return Err("Sub-issues are one level deep in v1".to_string());
            }
            let Some(parent_epic) = parent_epic else {
                return Err("Parent issue must belong to an epic".to_string());
            };
            if input.epic_id.as_deref().is_some_and(|id| id != parent_epic) {
                return Err("Sub-issue epic must match parent issue epic".to_string());
            }
            parent_epic
        } else {
            let Some(epic_id) = input.epic_id.as_deref() else {
                return Err("Issue must belong to an epic".to_string());
            };
            ensure_epic_exists(&tx, epic_id)?;
            epic_id.to_string()
        };

        let id = next_id(&tx, "issue", "issue_prefix")?;
        let now = Utc::now().to_rfc3339();
        let tags_json =
            serde_json::to_string(&input.tags).map_err(|e| format!("Invalid tags: {}", e))?;
        tx.execute(
            "INSERT INTO issue_items(id, epic_id, parent_issue_id, title, description, status, priority, assignee_kind, assignee_id, created_by, updated_by, tags_json, context_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11, ?12, ?13, ?13)",
            params![id, epic_id, input.parent_issue_id, input.title, input.description, status, priority, assignee_kind, input.assignee_id, actor, tags_json, input.context_json, now],
        )
        .map_err(|e| format!("Failed to create issue: {}", e))?;
        append_event(
            &tx,
            actor,
            "create",
            "issue",
            &id,
            Some(json!({"id": id}).to_string()),
            None,
        )?;
        tx.commit()
            .map_err(|e| format!("Failed to commit issue create: {}", e))?;
        drop(conn);

        self.get(project_path, &id)?
            .map(|d| d.issue)
            .ok_or_else(|| format!("Issue missing after create: {}", id))
    }

    /// Apply a partial update to an issue. `None` fields preserve the
    /// existing value; the new state is appended to the audit log.
    pub fn update(
        &self,
        project_path: &Path,
        issue_id: &str,
        patch: IssueUpdateInput,
    ) -> Result<Issue, String> {
        let actor = actor(patch.actor.as_deref());
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start issue update tx: {}", e))?;
        ensure_issue_exists(&tx, issue_id)?;
        if let Some(title) = patch.title.as_deref() {
            validate_title(title)?;
        }
        if let Some(status) = patch.status.as_deref() {
            validate_issue_status(status)?;
        }
        if let Some(priority) = patch.priority {
            validate_priority(priority)?;
        }
        if let Some(epic_id) = patch.epic_id.as_deref() {
            ensure_epic_exists(&tx, epic_id)?;
        }
        if patch.assignee_kind.is_some() || patch.assignee_id.is_some() {
            let kind = patch.assignee_kind.as_deref().unwrap_or("unassigned");
            validate_assignee(kind, patch.assignee_id.as_deref())?;
        }
        if let Some(tags) = patch.tags.as_ref() {
            validate_tags(tags)?;
        }

        let existing =
            get_issue(&tx, issue_id)?.ok_or_else(|| format!("Issue not found: {}", issue_id))?;
        let title = patch.title.unwrap_or(existing.title);
        let description = patch.description.or(existing.description);
        let status = patch.status.unwrap_or(existing.status);
        let priority = patch.priority.unwrap_or(existing.priority);
        let epic_id = patch.epic_id.or(existing.epic_id);
        let assignee_kind = patch.assignee_kind.unwrap_or(existing.assignee_kind);
        let assignee_id = patch.assignee_id.or(existing.assignee_id);
        let tags = patch.tags.unwrap_or(existing.tags);
        let tags_json = serde_json::to_string(&tags).map_err(|e| format!("Invalid tags: {}", e))?;
        let context_json = patch.context_json.or(existing.context_json);
        let now = Utc::now().to_rfc3339();

        tx.execute(
            "UPDATE issue_items SET title = ?1, description = ?2, status = ?3, priority = ?4, epic_id = ?5, assignee_kind = ?6, assignee_id = ?7, tags_json = ?8, context_json = ?9, updated_by = ?10, updated_at = ?11 WHERE id = ?12",
            params![title, description, status, priority, epic_id, assignee_kind, assignee_id, tags_json, context_json, actor, now, issue_id],
        )
        .map_err(|e| format!("Failed to update issue: {}", e))?;
        append_event(
            &tx,
            actor,
            "update",
            "issue",
            issue_id,
            None,
            Some(json!({"updated": true}).to_string()),
        )?;
        tx.commit()
            .map_err(|e| format!("Failed to commit issue update: {}", e))?;
        drop(conn);
        self.get(project_path, issue_id)?
            .map(|d| d.issue)
            .ok_or_else(|| format!("Issue missing after update: {}", issue_id))
    }

    /// Hard-delete an issue and any rows that cascade off of it
    /// (comments, dependency edges, sub-issues). Records a `delete`
    /// audit event before the row goes.
    pub fn delete(&self, project_path: &Path, issue_id: &str) -> Result<(), String> {
        let db = self.db(project_path)?;
        let mut conn = db.write();
        let tx = conn
            .transaction()
            .map_err(|e| format!("Failed to start issue delete tx: {}", e))?;
        ensure_issue_exists(&tx, issue_id)?;
        append_event(
            &tx,
            DEFAULT_ACTOR,
            "delete",
            "issue",
            issue_id,
            Some(json!({"id": issue_id}).to_string()),
            None,
        )?;
        tx.execute("DELETE FROM issue_items WHERE id = ?1", params![issue_id])
            .map_err(|e| format!("Failed to delete issue: {}", e))?;
        tx.commit()
            .map_err(|e| format!("Failed to commit issue delete: {}", e))
    }
}
