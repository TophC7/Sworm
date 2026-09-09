use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct OmpResolvedTarget {
    pub path: String,
    pub is_dir: bool,
}
