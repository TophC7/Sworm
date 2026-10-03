use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderInfo {
    pub path: String,
    pub name: String,
}

/// Where the browser's path bar starts: Home, or a volume (a
/// user-visible mount or the root filesystem) with its display label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathRoot {
    pub kind: PathRootKind,
    pub label: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathRootKind {
    Home,
    Volume,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}
