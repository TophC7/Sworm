use serde::Serialize;

#[derive(Serialize)]
pub struct FolderInfo {
    pub path: String,
    pub name: String,
}

#[derive(Serialize)]
pub struct FolderEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}
