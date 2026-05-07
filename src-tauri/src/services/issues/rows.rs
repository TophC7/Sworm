//! Row-to-DTO mappers for the issue store.

use crate::models::issues::*;

pub(super) fn row_to_issue(row: &rusqlite::Row<'_>) -> rusqlite::Result<Issue> {
    let tags_json: String = row.get(11)?;
    let tags = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(Issue {
        id: row.get(0)?,
        epic_id: row.get(1)?,
        parent_issue_id: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        status: row.get(5)?,
        priority: row.get(6)?,
        assignee_kind: row.get(7)?,
        assignee_id: row.get(8)?,
        created_by: row.get(9)?,
        updated_by: row.get(10)?,
        tags,
        context_json: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

pub(super) fn row_to_epic(row: &rusqlite::Row<'_>) -> rusqlite::Result<IssueEpic> {
    Ok(IssueEpic {
        id: row.get(0)?,
        title: row.get(1)?,
        description: row.get(2)?,
        status: row.get(3)?,
        priority: row.get(4)?,
        created_by: row.get(5)?,
        updated_by: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

pub(super) fn row_to_comment(row: &rusqlite::Row<'_>) -> rusqlite::Result<IssueComment> {
    Ok(IssueComment {
        id: row.get(0)?,
        issue_id: row.get(1)?,
        author: row.get(2)?,
        body: row.get(3)?,
        created_by: row.get(4)?,
        updated_by: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

pub(super) fn row_to_dependency(row: &rusqlite::Row<'_>) -> rusqlite::Result<IssueDependency> {
    Ok(IssueDependency {
        id: row.get(0)?,
        issue_id: row.get(1)?,
        depends_on_issue_id: row.get(2)?,
        created_by: row.get(3)?,
        created_at: row.get(4)?,
    })
}

pub(super) fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<IssueEvent> {
    Ok(IssueEvent {
        id: row.get(0)?,
        actor: row.get(1)?,
        action: row.get(2)?,
        entity_type: row.get(3)?,
        entity_id: row.get(4)?,
        snapshot_json: row.get(5)?,
        changes_json: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub(super) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
    label: &str,
) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    for row in rows {
        match row {
            Ok(item) => out.push(item),
            Err(error) => tracing::warn!("Dropping unreadable {} row: {}", label, error),
        }
    }
    Ok(out)
}
