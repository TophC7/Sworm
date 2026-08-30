use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    ClaudeCode,
    Codex,
    Omp,
    Gemini,
    Fresh,
    Terminal,
}

impl ProviderId {
    pub const fn as_str(&self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "claude_code",
            ProviderId::Codex => "codex",
            ProviderId::Omp => "omp",
            ProviderId::Gemini => "gemini",
            ProviderId::Fresh => "fresh",
            ProviderId::Terminal => "terminal",
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
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
    /// Simple flag-based resume (e.g. `--resume latest` for Gemini).
    /// Flags are appended verbatim on restart; ignored on first start.
    GenericFlag {
        flags: &'static [&'static str],
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SessionIdMode {
    None,
    Deterministic { flag: &'static str },
}
