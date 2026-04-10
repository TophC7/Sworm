use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub default_branch: Option<String>,
    pub base_ref: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
