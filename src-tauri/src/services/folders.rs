use crate::errors::ApiError;
use std::path::{Component, Path, PathBuf};

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

/// Normalize an absolute path lexically without resolving symlinks.
pub fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                ) {
                    normalized.pop();
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
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
