//! Method router: turns an authenticated [`BridgeRequest`] into a
//! [`BridgeResponse`] by invoking the matching [`IssueService`] call
//! and translating any error message into a stable bridge error code.

use super::protocol::{
    classify_error, optional_i64, required_string, to_value, BridgeRequest, BridgeResponse,
    PROTOCOL_VERSION,
};
use crate::models::issues::{
    IssueCommentCreateInput, IssueCreateInput, IssueDependencyInput, IssueSearchFilters,
    IssueUpdateInput,
};
use crate::services::issues::IssueService;
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn handle_request(
    request: BridgeRequest,
    issues: &IssueService,
    project_id: &str,
    project_path: &Path,
    token: &str,
) -> BridgeResponse {
    let id = request.id.clone();
    if request.token.as_deref() != Some(token) {
        return BridgeResponse::error(id, "unauthorized", "Invalid Sworm issue bridge token");
    }

    let result: Result<Value, String> = (|| match request.method.as_str() {
        "bridge.info" => Ok(json!({
            "protocol_version": PROTOCOL_VERSION,
            "project_id": project_id,
            "capabilities": ["issues.v1"]
        })),
        "issue.ready" => {
            let limit = optional_i64(&request.params, "limit");
            issues.ready(project_path, limit).map(to_value)
        }
        "issue.search" => {
            let query = required_string(&request.params, "query")?;
            let filters = serde_json::from_value::<IssueSearchFilters>(
                request
                    .params
                    .get("filters")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default())),
            )
            .map_err(|e| e.to_string())?;
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
            let input = serde_json::from_value::<IssueCreateInput>(request.params)
                .map_err(|e| e.to_string())?;
            issues.create(project_path, input).map(to_value)
        }
        "issue.update" => {
            let issue_id = required_string(&request.params, "issueId")?;
            let patch = serde_json::from_value::<IssueUpdateInput>(
                request
                    .params
                    .get("patch")
                    .cloned()
                    .unwrap_or(Value::Object(Default::default())),
            )
            .map_err(|e| e.to_string())?;
            issues.update(project_path, &issue_id, patch).map(to_value)
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
        "issue.comment.add" => {
            let input = serde_json::from_value::<IssueCommentCreateInput>(request.params)
                .map_err(|e| e.to_string())?;
            issues.add_comment(project_path, input).map(to_value)
        }
        "issue.dependency.add" => {
            let input = serde_json::from_value::<IssueDependencyInput>(request.params)
                .map_err(|e| e.to_string())?;
            issues.add_dependency(project_path, input).map(to_value)
        }
        "issue.dependency.remove" => {
            let input = serde_json::from_value::<IssueDependencyInput>(request.params)
                .map_err(|e| e.to_string())?;
            issues
                .remove_dependency(project_path, input)
                .map(|_| json!({}))
        }
        _ => Err(format!("Unknown issue bridge method: {}", request.method)),
    })();

    match result {
        Ok(value) => BridgeResponse::ok(id, value),
        Err(message) => {
            let code = classify_error(&message);
            BridgeResponse::error(id, code, &message)
        }
    }
}
