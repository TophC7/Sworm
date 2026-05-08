//! Serialized DTOs for the issue subsystem.
//!
//! These types cross both the Tauri command boundary and the
//! Unix-socket bridge, so all of them are `#[serde(rename_all = "camelCase")]`
//! and avoid backend-only fields. Field meanings match the columns in
//! `services::issues::SCHEMA_SQL`.

use serde::{Deserialize, Serialize};

/// Top-level grouping for issues. Roughly: an epic is a body of work
/// that a session can pick up across multiple issues.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueEpic {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: i64,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A single issue. Always belongs to an epic; sub-issues additionally
/// reference a parent issue and inherit that parent's epic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub id: String,
    pub epic_id: Option<String>,
    pub parent_issue_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: i64,
    pub assignee_kind: String,
    pub assignee_id: Option<String>,
    pub created_by: String,
    pub updated_by: String,
    pub tags: Vec<String>,
    pub context_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Free-form note attached to an issue. Authors agents and humans
/// alike use this for durable findings, blockers, or completion notes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueComment {
    pub id: String,
    pub issue_id: String,
    pub author: String,
    pub body: String,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Edge in the issue dependency graph: `issue_id` depends on
/// `depends_on_issue_id`. Cycles are rejected at insert time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDependency {
    pub id: String,
    pub issue_id: String,
    pub depends_on_issue_id: String,
    pub created_by: String,
    pub created_at: String,
}

/// Append-only audit event. Every mutation writes one of these so the
/// activity timeline survives across sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueEvent {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub entity_type: String,
    pub entity_id: String,
    pub snapshot_json: Option<String>,
    pub changes_json: Option<String>,
    pub created_at: String,
}

/// Single-issue lookup payload: the issue plus everything a detail
/// surface needs to render without a follow-up round trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    pub issue: Issue,
    pub comments: Vec<IssueComment>,
    pub depends_on: Vec<IssueDependency>,
    pub blocked_by: Vec<IssueDependency>,
    pub sub_issues: Vec<Issue>,
    pub events: Vec<IssueEvent>,
}

/// Optional filters for `issues_list`. All fields are independent.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IssueListFilters {
    pub status: Option<String>,
    pub epic_id: Option<String>,
    pub include_archived: Option<bool>,
    pub limit: Option<i64>,
}

/// Optional filters for `issues_ready`. Ready issues are always top-level
/// `todo` issues with no unresolved dependencies; these fields only scope
/// and page that queue.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IssueReadyFilters {
    pub epic_id: Option<String>,
    pub limit: Option<i64>,
}

/// Optional filters for `issues_search`. The query string itself is a
/// separate parameter; this struct only carries refinements.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IssueSearchFilters {
    pub status: Option<String>,
    pub epic_id: Option<String>,
    pub include_archived: Option<bool>,
    pub limit: Option<i64>,
}

/// Input for `issues_create`. Either `epic_id` or `parent_issue_id`
/// must be set; sub-issues inherit their parent's epic.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueCreateInput {
    pub title: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
    pub epic_id: Option<String>,
    pub parent_issue_id: Option<String>,
    pub assignee_kind: Option<String>,
    pub assignee_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub context_json: Option<String>,
    pub actor: Option<String>,
}

/// Patch payload for `issues_update`. Only `Some` fields are written;
/// `None` leaves the existing value untouched.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IssueUpdateInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
    pub epic_id: Option<String>,
    pub assignee_kind: Option<String>,
    pub assignee_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub context_json: Option<String>,
    pub actor: Option<String>,
}

/// Input for `issue_epics_create`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueEpicCreateInput {
    pub title: String,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
    pub actor: Option<String>,
}

/// Patch payload for `issue_epics_update`. Only `Some` fields apply.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct IssueEpicUpdateInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub priority: Option<i64>,
    pub actor: Option<String>,
}

/// Input for `issue_comments_add`. `author` is the human/agent label
/// shown in the timeline; `actor` records who performed the call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommentCreateInput {
    pub issue_id: String,
    pub author: String,
    pub body: String,
    pub actor: Option<String>,
}

/// Patch payload for `issue_comments_update`. Only the body changes;
/// to delete a comment use `issue_comments_delete`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommentUpdateInput {
    pub body: String,
    pub actor: Option<String>,
}

/// Input for both add- and remove-dependency commands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDependencyInput {
    pub issue_id: String,
    pub depends_on_issue_id: String,
    pub actor: Option<String>,
}

/// Single key/value row from the project's `issue_config` table.
/// Keys are restricted (`issue_prefix` / `epic_prefix` / `comment_prefix`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueConfigEntry {
    pub key: String,
    pub value: String,
}
