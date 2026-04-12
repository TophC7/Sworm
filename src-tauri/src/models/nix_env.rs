use serde::{Deserialize, Serialize};

/// Status of a Nix environment evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NixEnvStatus {
    Pending,
    Evaluating,
    Ready,
    Error,
    Timeout,
}

impl NixEnvStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Evaluating => "evaluating",
            Self::Ready => "ready",
            Self::Error => "error",
            Self::Timeout => "timeout",
        }
    }

    pub fn from_db_str(s: &str) -> Self {
        match s {
            "pending" => Self::Pending,
            "evaluating" => Self::Evaluating,
            "ready" => Self::Ready,
            "error" => Self::Error,
            "timeout" => Self::Timeout,
            _ => Self::Error,
        }
    }
}

/// Persisted Nix environment record for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NixEnvRecord {
    pub project_id: String,
    pub nix_file: String,
    pub status: NixEnvStatus,
    #[serde(skip_serializing)]
    pub env_json: Option<String>,
    pub error_message: Option<String>,
    pub evaluated_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Result of detecting Nix files for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NixDetection {
    pub project_id: String,
    pub project_path: String,
    pub detected_files: Vec<String>,
    pub selected: Option<NixEnvRecord>,
}
