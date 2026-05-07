//! NDJSON RPC envelope: request/response shape, error classification,
//! and parameter helpers shared by the dispatch table.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(super) const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub(super) struct BridgeRequest {
    pub(super) id: Option<String>,
    pub(super) token: Option<String>,
    pub(super) method: String,
    #[serde(default)]
    pub(super) params: Value,
}

#[derive(Debug, Serialize)]
pub(super) struct BridgeResponse {
    pub(super) id: Option<String>,
    pub(super) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<BridgeError>,
}

#[derive(Debug, Serialize)]
pub(super) struct BridgeError {
    pub(super) code: String,
    pub(super) message: String,
}

impl BridgeResponse {
    pub(super) fn ok(id: Option<String>, result: Value) -> Self {
        Self {
            id,
            ok: true,
            result: Some(result),
            error: None,
        }
    }

    pub(super) fn error(id: Option<String>, code: &str, message: &str) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(BridgeError {
                code: code.to_string(),
                message: message.to_string(),
            }),
        }
    }
}

/// Pick a stable error code for the bridge envelope based on the
/// service-layer message shape. Mirrors `commands::issues::map_issue_error`
/// so the Pi client can branch on the same categories the frontend sees.
pub(super) fn classify_error(message: &str) -> &'static str {
    if message.contains("not found")
        || message.contains("not Found")
        || message.contains("Not found")
    {
        "not_found"
    } else if is_validation_message(message) {
        "invalid_argument"
    } else {
        "domain_error"
    }
}

fn is_validation_message(message: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "must not be empty",
        "must be between",
        "Invalid issue status",
        "Invalid epic status",
        "Invalid assignee kind",
        "assigneeId required",
        "Invalid issue config key",
        "Prefix must",
        "Sub-issues are one level deep",
        "must belong to an epic",
        "Sub-issue epic must match",
        "cannot depend on itself",
        "already exists",
        "would create a cycle",
        "Cannot delete epic while it has issues",
        "Tags must not be empty",
        "Issue must belong",
        "Value must not",
    ];
    PATTERNS.iter().any(|p| message.contains(p))
}

pub(super) fn to_value<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

pub(super) fn required_string(params: &Value, key: &str) -> Result<String, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| format!("Missing required param: {}", key))
}

pub(super) fn optional_i64(params: &Value, key: &str) -> Option<i64> {
    params.get(key).and_then(Value::as_i64)
}
