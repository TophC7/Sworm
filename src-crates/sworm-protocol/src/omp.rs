use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmpResolvedTarget {
    pub path: String,
    pub is_dir: bool,
}
