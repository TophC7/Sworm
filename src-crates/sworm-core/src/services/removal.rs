//! Removing user files: OS trash by default, permanent only when asked.
use crate::errors::ApiError;
use std::path::{Path, PathBuf};

/// Move `paths` to the OS trash, or delete them outright when `permanent`.
/// Callers pass paths that exist; a trash failure is `TrashUnavailable`.
pub(crate) fn remove_paths(paths: &[PathBuf], permanent: bool) -> Result<(), ApiError> {
    if paths.is_empty() {
        return Ok(());
    }
    if !permanent {
        return trash::delete_all(paths).map_err(|error| ApiError::TrashUnavailable {
            message: format!("Could not move to the trash: {error}"),
        });
    }
    paths.iter().try_for_each(|path| remove_recursive(path))
}

/// Recursively remove a file or directory.
pub(crate) fn remove_recursive(path: &Path) -> Result<(), ApiError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| ApiError::Io(format!("Cannot stat {}: {}", path.display(), e)))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|e| ApiError::Io(format!("Cannot remove {}: {}", path.display(), e)))
    } else {
        std::fs::remove_file(path)
            .map_err(|e| ApiError::Io(format!("Cannot remove {}: {}", path.display(), e)))
    }
}
