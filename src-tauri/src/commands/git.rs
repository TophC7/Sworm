use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::git::{GitCommit, GitSummary};
use std::path::{Path, PathBuf};

/// Get git summary for a project path.
#[tauri::command]
pub fn git_get_summary(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<GitSummary, ApiError> {
    Ok(state.git.get_summary(Path::new(&path)))
}

/// Get diff for a specific file.
#[tauri::command]
pub fn git_get_file_diff(
    project_path: String,
    file_path: String,
    staged: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    let root = PathBuf::from(&project_path)
        .canonicalize()
        .map_err(|e| ApiError::InvalidArgument(format!("Invalid project path: {}", e)))?;
    let candidate = root.join(&file_path);
    let normalized = candidate
        .canonicalize()
        .or_else(|_| candidate.parent().unwrap_or(&candidate).canonicalize().map(|parent| parent.join(candidate.file_name().unwrap_or_default())))
        .map_err(|e| ApiError::InvalidArgument(format!("Invalid file path: {}", e)))?;

    if !normalized.starts_with(&root) {
        return Err(ApiError::InvalidArgument(
            "File path must stay within the project root".to_string(),
        ));
    }

    Ok(state
        .git
        .get_file_diff(Path::new(&project_path), &file_path, staged))
}

/// Get recent commits for the current branch.
#[tauri::command]
pub fn git_get_log(
    path: String,
    limit: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GitCommit>, ApiError> {
    Ok(state.git.get_log(Path::new(&path), limit))
}
