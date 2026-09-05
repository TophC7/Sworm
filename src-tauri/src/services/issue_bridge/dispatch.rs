//! Method router: turns an authenticated [`BridgeRequest`] into a
//! [`BridgeResponse`] by invoking the matching [`IssueService`] call
//! and translating any error message into a stable bridge error code.

use super::protocol::{
    classify_error, optional_i64, required_string, to_value, BridgeRequest, BridgeResponse,
    PROTOCOL_VERSION,
};
use crate::models::issues::{
    IssueCommentCreateInput, IssueCommentUpdateInput, IssueCreateInput, IssueDependencyInput,
    IssueEpicCreateInput, IssueEpicUpdateInput, IssueListFilters, IssueReadyFilters,
    IssueSearchFilters, IssueUpdateInput,
};
use crate::services::issues::IssueService;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;
use tauri::Emitter;

const BRIDGE_METHODS: &[&str] = &[
    "bridge.info",
    "epic.create",
    "epic.list",
    "epic.show",
    "epic.update",
    "epic.delete",
    "issue.list",
    "issue.ready",
    "issue.search",
    "issue.show",
    "issue.create",
    "issue.update",
    "issue.delete",
    "issue.claim",
    "comment.add",
    "comment.list",
    "comment.update",
    "comment.delete",
    "dependency.add",
    "dependency.remove",
    "dependency.list",
    "config.list",
    "config.get",
    "config.set",
];

pub(super) fn handle_request(
    request: BridgeRequest,
    issues: &IssueService,
    app: Option<&tauri::AppHandle>,
    project_path: &Path,
    token: &str,
) -> BridgeResponse {
    let id = request.id.clone();
    if request.token.as_deref() != Some(token) {
        return BridgeResponse::error(id, "unauthorized", "Invalid Sworm issue bridge token");
    }
    let mutation = is_mutation(&request.method);

    let result: Result<Value, String> = (|| match request.method.as_str() {
        "bridge.info" => Ok(json!({
            "protocol_version": PROTOCOL_VERSION,
            "project_path": project_path.to_string_lossy(),
            "capabilities": ["issues.v1", "issues.full.v1"],
            "methods": BRIDGE_METHODS
        })),

        "epic.create" => {
            let input = parse::<IssueEpicCreateInput>(request.params)?;
            issues.create_epic(project_path, input).map(to_value)
        }
        "epic.list" => issues.list_epics(project_path).map(to_value),
        "epic.show" => {
            let epic_id = required_string(&request.params, "epicId")?;
            issues
                .get_epic(project_path, &epic_id)
                .and_then(|item| item.ok_or_else(|| format!("Epic not found: {}", epic_id)))
                .map(to_value)
        }
        "epic.update" => {
            let epic_id = required_string(&request.params, "epicId")?;
            let patch = parse_param::<IssueEpicUpdateInput>(&request.params, "patch")?;
            issues
                .update_epic(project_path, &epic_id, patch)
                .map(to_value)
        }
        "epic.delete" => {
            let epic_id = required_string(&request.params, "epicId")?;
            issues
                .delete_epic(project_path, &epic_id)
                .map(|_| json!({}))
        }

        "issue.list" => {
            let filters = parse_param::<IssueListFilters>(&request.params, "filters")?;
            issues.list(project_path, filters).map(to_value)
        }
        "issue.ready" => {
            let mut filters = parse_param::<IssueReadyFilters>(&request.params, "filters")?;
            if filters.limit.is_none() {
                filters.limit = optional_i64(&request.params, "limit");
            }
            issues.ready(project_path, filters).map(to_value)
        }
        "issue.search" => {
            let query = required_string(&request.params, "query")?;
            let filters = parse_param::<IssueSearchFilters>(&request.params, "filters")?;
            issues.search(project_path, &query, filters).map(to_value)
        }
        "issue.show" => {
            let issue_id = required_string(&request.params, "issueId")?;
            issues
                .get(project_path, &issue_id)
                .and_then(|item| item.ok_or_else(|| format!("Issue not found: {}", issue_id)))
                .map(to_value)
        }
        "issue.create" => {
            let input = parse::<IssueCreateInput>(request.params)?;
            issues.create(project_path, input).map(to_value)
        }
        "issue.update" => {
            let issue_id = required_string(&request.params, "issueId")?;
            let patch = parse_param::<IssueUpdateInput>(&request.params, "patch")?;
            issues.update(project_path, &issue_id, patch).map(to_value)
        }
        "issue.delete" => {
            let issue_id = required_string(&request.params, "issueId")?;
            issues.delete(project_path, &issue_id).map(|_| json!({}))
        }
        "issue.claim" => {
            let issue_id = required_string(&request.params, "issueId")?;
            let assignee_kind = request
                .params
                .get("assigneeKind")
                .and_then(Value::as_str)
                .unwrap_or("agent")
                .to_string();
            let assignee_id = request
                .params
                .get("assigneeId")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let actor = request
                .params
                .get("actor")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            issues
                .update(
                    project_path,
                    &issue_id,
                    IssueUpdateInput {
                        status: Some("in_progress".to_string()),
                        assignee_kind: Some(assignee_kind),
                        assignee_id,
                        actor,
                        ..Default::default()
                    },
                )
                .map(to_value)
        }

        "comment.add" | "issue.comment.add" => {
            let input = parse::<IssueCommentCreateInput>(request.params)?;
            issues.add_comment(project_path, input).map(to_value)
        }
        "comment.list" | "issue.comment.list" => {
            let issue_id = required_string(&request.params, "issueId")?;
            issues.list_comments(project_path, &issue_id).map(to_value)
        }
        "comment.update" | "issue.comment.update" => {
            let comment_id = required_string(&request.params, "commentId")?;
            let input = parse_required_param::<IssueCommentUpdateInput>(&request.params, "input")?;
            issues
                .update_comment(project_path, &comment_id, input)
                .map(to_value)
        }
        "comment.delete" | "issue.comment.delete" => {
            let comment_id = required_string(&request.params, "commentId")?;
            issues
                .delete_comment(project_path, &comment_id)
                .map(|_| json!({}))
        }

        "dependency.add" | "issue.dependency.add" => {
            let input = parse::<IssueDependencyInput>(request.params)?;
            issues.add_dependency(project_path, input).map(to_value)
        }
        "dependency.remove" | "issue.dependency.remove" => {
            let input = parse::<IssueDependencyInput>(request.params)?;
            issues
                .remove_dependency(project_path, input)
                .map(|_| json!({}))
        }
        "dependency.list" | "issue.dependency.list" => {
            let issue_id = required_string(&request.params, "issueId")?;
            issues
                .list_dependencies(project_path, &issue_id)
                .map(to_value)
        }

        "config.list" | "issue.config.list" => issues.list_config(project_path).map(to_value),
        "config.get" | "issue.config.get" => {
            let key = required_string(&request.params, "key")?;
            issues.get_config(project_path, &key).map(to_value)
        }
        "config.set" | "issue.config.set" => {
            let key = required_string(&request.params, "key")?;
            let value = required_string(&request.params, "value")?;
            issues.set_config(project_path, &key, &value).map(to_value)
        }
        _ => Err(format!("Unknown issue bridge method: {}", request.method)),
    })();

    match result {
        Ok(value) => {
            if mutation {
                if let Some(app) = app {
                    if let Err(error) = app.emit(
                        "issues-changed",
                        json!({ "folderPath": project_path.to_string_lossy() }),
                    ) {
                        tracing::warn!("Failed to emit issues-changed: {}", error);
                    }
                }
            }
            BridgeResponse::ok(id, value)
        }
        Err(message) => {
            let code = classify_error(&message);
            BridgeResponse::error(id, code, &message)
        }
    }
}

fn is_mutation(method: &str) -> bool {
    matches!(
        method,
        "epic.create"
            | "epic.update"
            | "epic.delete"
            | "issue.create"
            | "issue.update"
            | "issue.delete"
            | "issue.claim"
            | "comment.add"
            | "issue.comment.add"
            | "comment.update"
            | "issue.comment.update"
            | "comment.delete"
            | "issue.comment.delete"
            | "dependency.add"
            | "issue.dependency.add"
            | "dependency.remove"
            | "issue.dependency.remove"
            | "config.set"
            | "issue.config.set"
    )
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value::<T>(value).map_err(|e| e.to_string())
}

fn parse_param<T: DeserializeOwned + Default>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value::<T>(
        params
            .get(key)
            .cloned()
            .unwrap_or(Value::Object(Default::default())),
    )
    .map_err(|e| e.to_string())
}

fn parse_required_param<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, String> {
    serde_json::from_value::<T>(
        params
            .get(key)
            .cloned()
            .ok_or_else(|| format!("Missing required param: {}", key))?,
    )
    .map_err(|e| e.to_string())
}
