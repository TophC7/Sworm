use crate::errors::ApiError;
use std::path::{Path, PathBuf};

/// Canonicalize a user-supplied folder path and require it to be a directory.
pub fn resolve_folder(path: &str) -> Result<PathBuf, ApiError> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| ApiError::NotFound(format!("Folder not found: {path}")))?;
    if !canonical.is_dir() {
        return Err(ApiError::InvalidArgument(format!(
            "Not a directory: {path}"
        )));
    }
    Ok(canonical)
}

/// Basename of a folder, falling back to the full path for roots like `/`.
pub fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The user's home directory from `$HOME`; provider state lives beneath it.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
