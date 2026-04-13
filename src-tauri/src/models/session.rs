use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_id: String,
    pub provider_id: String,
    pub title: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub status: String,
    pub shared_workspace: bool,
    pub auto_approve: bool,
    pub provider_resume_token: Option<String>,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_started_at: Option<String>,
    pub last_stopped_at: Option<String>,
}
