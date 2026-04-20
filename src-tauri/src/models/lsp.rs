use crate::services::settings::LspServerConfigRecord;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspExtensionManifest {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub languages: Vec<LspLanguageContribution>,
    #[serde(default)]
    pub servers: Vec<LspServerDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspLanguageContribution {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerDefinition {
    pub id: String,
    pub label: String,
    pub runtime: LspRuntimeDefinition,
    #[serde(default)]
    pub args: Vec<String>,
    pub install_hint: String,
    #[serde(default)]
    pub document_selectors: Vec<LspDocumentSelector>,
    #[serde(default)]
    pub initialization_options: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRuntimeDefinition {
    pub kind: LspRuntimeKind,
    pub command: String,
    #[serde(default)]
    pub runtime_command: Option<String>,
    #[serde(default)]
    pub runtime_args: Vec<String>,
    #[serde(default)]
    pub entry_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LspRuntimeKind {
    HostBinary,
    BundledBinary,
    ExtensionBinary,
    BundledJs,
    ExtensionJs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDocumentSelector {
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub filenames: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspExtensionEntry {
    pub id: String,
    pub label: String,
    pub built_in: bool,
    pub source_path: Option<String>,
    pub languages: Vec<LspLanguageContribution>,
    pub servers: Vec<LspServerCatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspServerCatalogEntry {
    pub server_definition_id: String,
    pub extension_id: String,
    pub extension_label: String,
    pub label: String,
    pub install_hint: String,
    pub runtime_kind: LspRuntimeKind,
    pub command: String,
    pub runtime_command: Option<String>,
    pub entry_path: Option<String>,
    pub args: Vec<String>,
    pub document_selectors: Vec<LspDocumentSelector>,
    pub initialization_options: Option<Value>,
    pub built_in: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspServerSettingsEntry {
    pub server: LspServerStatus,
    pub config: LspServerConfigRecord,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspServerStatus {
    pub server_definition_id: String,
    pub extension_id: String,
    pub extension_label: String,
    pub label: String,
    pub enabled: bool,
    pub status: LspServerConnectionStatus,
    pub resolved_path: Option<String>,
    pub runtime_resolved_path: Option<String>,
    pub message: Option<String>,
    pub install_hint: String,
    pub document_selectors: Vec<LspDocumentSelector>,
    pub initialization_options: Option<Value>,
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
