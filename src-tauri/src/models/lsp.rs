use crate::models::builtins::{BuiltinDocumentSelector, BuiltinLspServerSettingsDescriptor};
use crate::services::settings::LspServerConfigRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct LspServerSettingsEntry {
    pub server: LspServerStatus,
    pub config: LspServerConfigRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspServerStatus {
    pub server_definition_id: String,
    pub builtin_id: String,
    pub builtin_label: String,
    pub label: String,
    pub enabled: bool,
    pub status: LspServerConnectionStatus,
    pub resolved_path: Option<String>,
    pub runtime_resolved_path: Option<String>,
    pub message: Option<String>,
    pub install_hint: String,
    pub document_selectors: Vec<BuiltinDocumentSelector>,
    pub initialization_options: Option<Value>,
    pub settings: Option<BuiltinLspServerSettingsDescriptor>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LspServerConnectionStatus {
    Connected,
    Missing,
    Error,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspTransportTraceDirection {
    Incoming,
    Outgoing,
    Stderr,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LspEvent {
    Started {
        session_id: String,
        pid: Option<u32>,
        resolved_path: Option<String>,
        runtime_resolved_path: Option<String>,
    },
    Message {
        session_id: String,
        payload_json: String,
    },
    Trace {
        session_id: String,
        direction: LspTransportTraceDirection,
        payload: String,
    },
    Exit {
        session_id: String,
        code: Option<i32>,
    },
    Error {
        session_id: String,
        message: String,
    },
}
