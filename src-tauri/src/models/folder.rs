use serde::Serialize;

#[derive(Serialize)]
pub struct FolderInfo {
    pub path: String,
    pub name: String,
}
