//! Method router: turns an authenticated [`BridgeRequest`] into a
//! [`BridgeResponse`] by invoking the matching [`IssueService`] call
//! and translating typed errors into stable bridge error codes.

use super::protocol::{
    classify_error, optional_i64, required_string, to_value, BridgeRequest, BridgeResponse,
    PROTOCOL_VERSION,
};
use crate::errors::ApiError;
use crate::events::{deliver, EventSink, HostEvent};
use crate::services::issues::IssueService;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::path::Path;
use sworm_protocol::issues::{
    IssueCommentCreateInput, IssueCommentUpdateInput, IssueCreateInput, IssueDependencyInput,
    IssueEpicCreateInput, IssueEpicUpdateInput, IssueListFilters, IssueReadyFilters,
    IssueUpdateInput,
};

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
    events: &EventSink<HostEvent>,
    project_path: &Path,
    token: &str,
) -> BridgeResponse {
    let id = request.id.clone();
    if request.token.as_deref() != Some(token) {
        return BridgeResponse::error(id, "unauthorized", "Invalid Sworm issue bridge token");
    }

    let result: Result<(Value, bool), ApiError> = (|| {
        let (result, mutated) = match request.method.as_str() {
            "bridge.info" => (
                Ok(json!({
                    "protocol_version": PROTOCOL_VERSION,
                    "project_path": project_path.to_string_lossy(),
                    "capabilities": ["issues.v1", "issues.full.v1"],
                    "methods": BRIDGE_METHODS
                })),
                false,
            ),

            "epic.create" => {
                let input = parse::<IssueEpicCreateInput>(request.params)?;
                (issues.create_epic(project_path, input).map(to_value), true)
            }
            "epic.list" => (issues.list_epics(project_path).map(to_value), false),
            "epic.show" => {
                let epic_id = required_string(&request.params, "epicId")?;
                (issues.get_epic(project_path, &epic_id).map(to_value), false)
            }
            "epic.update" => {
                let epic_id = required_string(&request.params, "epicId")?;
                let patch = parse_param::<IssueEpicUpdateInput>(&request.params, "patch")?;
                (
                    issues
                        .update_epic(project_path, &epic_id, patch)
                        .map(to_value),
                    true,
                )
            }
            "epic.delete" => {
                let epic_id = required_string(&request.params, "epicId")?;
                (
                    issues
                        .delete_epic(project_path, &epic_id)
                        .map(|_| json!({})),
                    true,
                )
            }

            "issue.list" => {
                let filters = parse_param::<IssueListFilters>(&request.params, "filters")?;
                (issues.list(project_path, filters).map(to_value), false)
            }
            "issue.ready" => {
                let mut filters = parse_param::<IssueReadyFilters>(&request.params, "filters")?;
                if filters.limit.is_none() {
                    filters.limit = optional_i64(&request.params, "limit");
                }
                (issues.ready(project_path, filters).map(to_value), false)
            }
            "issue.search" => {
                let query = required_string(&request.params, "query")?;
                let filters = parse_param::<IssueListFilters>(&request.params, "filters")?;
                (
                    issues.search(project_path, &query, filters).map(to_value),
                    false,
                )
            }
            "issue.show" => {
                let issue_id = required_string(&request.params, "issueId")?;
                (issues.get(project_path, &issue_id).map(to_value), false)
            }
            "issue.create" => {
                let input = parse::<IssueCreateInput>(request.params)?;
                (issues.create(project_path, input).map(to_value), true)
            }
            "issue.update" => {
                let issue_id = required_string(&request.params, "issueId")?;
                let patch = parse_param::<IssueUpdateInput>(&request.params, "patch")?;
                (
                    issues.update(project_path, &issue_id, patch).map(to_value),
                    true,
                )
            }
            "issue.delete" => {
                let issue_id = required_string(&request.params, "issueId")?;
                (
                    issues.delete(project_path, &issue_id).map(|_| json!({})),
                    true,
                )
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
                (
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
                        .map(to_value),
                    true,
                )
            }

            "comment.add" => {
                let input = parse::<IssueCommentCreateInput>(request.params)?;
                (issues.add_comment(project_path, input).map(to_value), true)
            }
            "comment.list" => {
                let issue_id = required_string(&request.params, "issueId")?;
                (
                    issues.list_comments(project_path, &issue_id).map(to_value),
                    false,
                )
            }
            "comment.update" => {
                let comment_id = required_string(&request.params, "commentId")?;
                let input =
                    parse_required_param::<IssueCommentUpdateInput>(&request.params, "input")?;
                (
                    issues
                        .update_comment(project_path, &comment_id, input)
                        .map(to_value),
                    true,
                )
            }
            "comment.delete" => {
                let comment_id = required_string(&request.params, "commentId")?;
                (
                    issues
                        .delete_comment(project_path, &comment_id)
                        .map(|_| json!({})),
                    true,
                )
            }

            "dependency.add" => {
                let input = parse::<IssueDependencyInput>(request.params)?;
                (
                    issues.add_dependency(project_path, input).map(to_value),
                    true,
                )
            }
            "dependency.remove" => {
                let input = parse::<IssueDependencyInput>(request.params)?;
                (
                    issues
                        .remove_dependency(project_path, input)
                        .map(|_| json!({})),
                    true,
                )
            }
            "dependency.list" => {
                let issue_id = required_string(&request.params, "issueId")?;
                (
                    issues
                        .list_dependencies(project_path, &issue_id)
                        .map(to_value),
                    false,
                )
            }

            "config.list" => (issues.list_config(project_path).map(to_value), false),
            "config.get" => {
                let key = required_string(&request.params, "key")?;
                (issues.get_config(project_path, &key).map(to_value), false)
            }
            "config.set" => {
                let key = required_string(&request.params, "key")?;
                let value = required_string(&request.params, "value")?;
                (
                    issues.set_config(project_path, &key, &value).map(to_value),
                    true,
                )
            }
            _ => (
                Err(ApiError::InvalidArgument(format!(
                    "Unknown issue bridge method: {}",
                    request.method
                ))),
                false,
            ),
        };
        result.map(|value| (value, mutated))
    })();

    match result {
        Ok((value, mutated)) => {
            if mutated {
                deliver(
                    events,
                    HostEvent::IssuesChanged(project_path.to_string_lossy().into_owned()),
                );
            }
            BridgeResponse::ok(id, value)
        }
        Err(error) => {
            let code = classify_error(&error);
            BridgeResponse::error(id, code, &error.to_string())
        }
    }
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ApiError> {
    serde_json::from_value::<T>(value).map_err(|e| ApiError::InvalidArgument(e.to_string()))
}

fn parse_param<T: DeserializeOwned + Default>(params: &Value, key: &str) -> Result<T, ApiError> {
    serde_json::from_value::<T>(
        params
            .get(key)
            .cloned()
            .unwrap_or(Value::Object(Default::default())),
    )
    .map_err(|e| ApiError::InvalidArgument(e.to_string()))
}

fn parse_required_param<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, ApiError> {
    serde_json::from_value::<T>(
        params
            .get(key)
            .cloned()
            .ok_or_else(|| ApiError::InvalidArgument(format!("Missing required param: {}", key)))?,
    )
    .map_err(|e| ApiError::InvalidArgument(e.to_string()))
}
