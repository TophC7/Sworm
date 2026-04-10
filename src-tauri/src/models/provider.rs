use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    ClaudeCode,
    Codex,
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderId::ClaudeCode => write!(f, "claude_code"),
            ProviderId::Codex => write!(f, "codex"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub id: ProviderId,
    pub label: String,
    pub status: ProviderConnectionStatus,
    pub version: Option<String>,
    pub resolved_path: Option<String>,
    pub message: Option<String>,
    pub install_hint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderConnectionStatus {
    Connected,
    Missing,
    Error,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum PromptMode {
    /// Append prompt text as the final argument.
    ArgvTail,
    /// Pass prompt via a named flag.
    FlagThenValue { flag: &'static str },
    /// Inject prompt after PTY start.
    KeystrokeInjection,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ResumeMode {
    None,
    SessionId {
        session_flag: &'static str,
        continue_flags: &'static [&'static str],
    },
    ThreadId {
        resume_command: &'static str,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SessionIdMode {
    None,
    Deterministic { flag: &'static str },
}
