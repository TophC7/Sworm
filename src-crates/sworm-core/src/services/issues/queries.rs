//! Connection-level SQL helpers shared by the op modules.
//!
//! Every function here takes a `&Connection` (or `&Transaction`) and
//! returns a `Result<_, ApiError>`. Helpers do not own connections; the
//! op modules borrow from [`super::IssueProjectDb::write`] /
//! [`super::IssueProjectDb::read`] and pass the guard through.

use super::db_error;
use super::rows::{
    collect_rows, row_to_comment, row_to_dependency, row_to_epic, row_to_event, row_to_issue,
    EPIC_COLUMNS, ISSUE_COLUMNS,
};
use crate::errors::ApiError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Params};
use std::collections::HashSet;
use sworm_protocol::issues::*;

pub(super) fn query_issues(
    conn: &Connection,
    sql: &str,
    params: impl Params,
) -> Result<Vec<Issue>, ApiError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(db_error("Failed to prepare issue query"))?;
    let rows = stmt
        .query_map(params, row_to_issue)
        .map_err(db_error("Failed to query issues"))?;
    collect_rows(rows, "issue")
}

pub(super) fn get_issue(conn: &Connection, issue_id: &str) -> Result<Issue, ApiError> {
    conn.query_row(
        &format!("SELECT {ISSUE_COLUMNS} FROM issue_items i WHERE i.id = ?1"),
        params![issue_id],
        row_to_issue,
    )
    .optional()
    .map_err(db_error("Failed to get issue"))?
    .ok_or_else(|| ApiError::NotFound(format!("Issue not found: {issue_id}")))
}

pub(super) fn ensure_issue_exists(conn: &Connection, issue_id: &str) -> Result<(), ApiError> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT i.id FROM issue_items i WHERE i.id = ?1",
            params![issue_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error("Failed to load issue"))?;
    exists
        .map(|_| ())
        .ok_or_else(|| ApiError::NotFound(format!("Issue not found: {}", issue_id)))
}

pub(super) fn ensure_epic_exists(conn: &Connection, epic_id: &str) -> Result<(), ApiError> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM issue_epics WHERE id = ?1",
            params![epic_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error("Failed to load epic"))?;
    exists
        .map(|_| ())
        .ok_or_else(|| ApiError::NotFound(format!("Epic not found: {}", epic_id)))
}

pub(super) fn get_epic_conn(conn: &Connection, epic_id: &str) -> Result<IssueEpic, ApiError> {
    conn.query_row(
        &format!("SELECT {EPIC_COLUMNS} FROM issue_epics e WHERE e.id = ?1"),
        params![epic_id],
        row_to_epic,
    )
    .optional()
    .map_err(db_error("Failed to get epic"))?
    .ok_or_else(|| ApiError::NotFound(format!("Epic not found: {epic_id}")))
}

pub(super) fn list_comments_conn(
    conn: &Connection,
    issue_id: &str,
) -> Result<Vec<IssueComment>, ApiError> {
    let mut stmt = conn.prepare("SELECT id, issue_id, author, body, created_by, updated_by, created_at, updated_at FROM issue_comments WHERE issue_id = ?1 ORDER BY created_at ASC")
        .map_err(db_error("Failed to prepare comment query"))?;
    let rows = stmt
        .query_map(params![issue_id], row_to_comment)
        .map_err(db_error("Failed to query comments"))?;
    collect_rows(rows, "comment")
}

pub(super) fn list_dependencies_conn(
    conn: &Connection,
    issue_id: &str,
    blocked_by: bool,
) -> Result<Vec<IssueDependency>, ApiError> {
    let sql = if blocked_by {
        "SELECT id, issue_id, depends_on_issue_id, created_by, created_at FROM issue_dependencies WHERE depends_on_issue_id = ?1 ORDER BY created_at ASC"
    } else {
        "SELECT id, issue_id, depends_on_issue_id, created_by, created_at FROM issue_dependencies WHERE issue_id = ?1 ORDER BY created_at ASC"
    };
    let mut stmt = conn
        .prepare(sql)
        .map_err(db_error("Failed to prepare dependency query"))?;
    let rows = stmt
        .query_map(params![issue_id], row_to_dependency)
        .map_err(db_error("Failed to query dependencies"))?;
    collect_rows(rows, "dependency")
}

pub(super) fn list_events_conn(
    conn: &Connection,
    entity_id: &str,
) -> Result<Vec<IssueEvent>, ApiError> {
    let mut stmt = conn.prepare("SELECT id, actor, action, entity_type, entity_id, snapshot_json, changes_json, created_at FROM issue_events WHERE entity_id = ?1 ORDER BY created_at DESC, id DESC LIMIT 100")
        .map_err(db_error("Failed to prepare event query"))?;
    let rows = stmt
        .query_map(params![entity_id], row_to_event)
        .map_err(db_error("Failed to query events"))?;
    collect_rows(rows, "event")
}

pub(super) fn append_event(
    conn: &Connection,
    actor: &str,
    action: &str,
    entity_type: &str,
    entity_id: &str,
    snapshot_json: Option<String>,
    changes_json: Option<String>,
) -> Result<(), ApiError> {
    let now = Utc::now().to_rfc3339();
    conn.execute("INSERT INTO issue_events(actor, action, entity_type, entity_id, snapshot_json, changes_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![actor, action, entity_type, entity_id, snapshot_json, changes_json, now])
        .map_err(db_error("Failed to append issue event"))?;
    Ok(())
}

pub(super) fn next_id(conn: &Connection, entity_type: &str) -> Result<String, ApiError> {
    conn.execute(
        "UPDATE issue_id_counters SET counter = counter + 1 WHERE entity_type = ?1",
        params![entity_type],
    )
    .map_err(db_error("Failed to increment issue counter"))?;
    let counter: i64 = conn
        .query_row(
            "SELECT counter FROM issue_id_counters WHERE entity_type = ?1",
            params![entity_type],
            |row| row.get(0),
        )
        .map_err(db_error("Failed to read issue counter"))?;
    let prefix: String = conn
        .query_row(
            "SELECT value FROM issue_config WHERE key = ?1",
            params![format!("{entity_type}_prefix")],
            |row| row.get(0),
        )
        .map_err(db_error("Failed to read issue prefix"))?;
    Ok(format!("{}-{}", prefix, counter))
}

pub(super) fn would_create_cycle(
    conn: &Connection,
    issue_id: &str,
    depends_on_issue_id: &str,
) -> Result<bool, ApiError> {
    let mut visited = HashSet::new();
    let mut stack = vec![depends_on_issue_id.to_string()];
    while let Some(current) = stack.pop() {
        if current == issue_id {
            return Ok(true);
        }
        if !visited.insert(current.clone()) {
            continue;
        }
        let mut stmt = conn
            .prepare("SELECT depends_on_issue_id FROM issue_dependencies WHERE issue_id = ?1")
            .map_err(db_error("Failed to prepare dependency traversal"))?;
        let rows = stmt
            .query_map(params![current], |row| row.get::<_, String>(0))
            .map_err(db_error("Failed to traverse dependencies"))?;
        for row in rows {
            stack.push(row.map_err(db_error("Failed to read dependency traversal row"))?);
        }
    }
    Ok(false)
}

pub(super) fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' | '_' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}
