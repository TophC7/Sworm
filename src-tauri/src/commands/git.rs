use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::git::{CommitDetail, DiffContext, GitSummary, GraphCommit};
use std::path::{Path, PathBuf};

/// Reject anything that isn't a hex commit hash (40-char full or 7+ short).
fn validated_git_ref(hash: &str) -> Result<(), ApiError> {
    if hash.len() >= 7 && hash.len() <= 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ApiError::InvalidArgument(format!(
            "Invalid git ref: {}",
            hash
        )))
    }
}

/// Validate that `file_path` stays within `project_path` after canonicalization.
///
/// Returns `Ok(())` on success — callers pass the original paths to the
/// service layer (git CLI resolves them relative to its `current_dir`).
fn validated_project_file(project_path: &str, file_path: &str) -> Result<(), ApiError> {
    let root = PathBuf::from(project_path)
        .canonicalize()
        .map_err(|e| ApiError::InvalidArgument(format!("Invalid project path: {}", e)))?;
    let candidate = root.join(file_path);
    let normalized = candidate
        .canonicalize()
        .or_else(|_| {
            candidate
                .parent()
                .unwrap_or(&candidate)
                .canonicalize()
                .map(|parent| parent.join(candidate.file_name().unwrap_or_default()))
        })
        .map_err(|e| ApiError::InvalidArgument(format!("Invalid file path: {}", e)))?;

    if !normalized.starts_with(&root) {
        return Err(ApiError::InvalidArgument(
            "File path must stay within the project root".to_string(),
        ));
    }

    Ok(())
}

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
    validated_project_file(&project_path, &file_path)?;
    Ok(state
        .git
        .get_file_diff(Path::new(&project_path), &file_path, staged))
}

/// Get structured diff context with file content for the diff viewer.
#[tauri::command]
pub fn git_get_diff_context(
    project_path: String,
    file_path: String,
    staged: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Option<DiffContext>, ApiError> {
    validated_project_file(&project_path, &file_path)?;
    Ok(state
        .git
        .get_diff_context(Path::new(&project_path), &file_path, staged))
}

/// Get full commit detail (metadata + file list with stats).
#[tauri::command]
pub fn git_get_commit_detail(
    path: String,
    hash: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommitDetail>, ApiError> {
    validated_git_ref(&hash)?;
    Ok(state.git.get_commit_detail(Path::new(&path), &hash))
}

/// Get all working-tree diffs in a single batch call (staged or unstaged).
#[tauri::command]
pub fn git_get_working_diffs(
    path: String,
    staged: bool,
    untracked_paths: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<std::collections::HashMap<String, String>, ApiError> {
    Ok(state
        .git
        .get_working_diffs(Path::new(&path), staged, &untracked_paths))
}

/// Get all file diffs for a commit in a single batch call.
#[tauri::command]
pub fn git_get_commit_diffs(
    path: String,
    hash: String,
    state: tauri::State<'_, AppState>,
) -> Result<std::collections::HashMap<String, String>, ApiError> {
    validated_git_ref(&hash)?;
    Ok(state.git.get_commit_diffs(Path::new(&path), &hash))
}

/// Get commit graph data for visualization (all branches).
#[tauri::command]
pub fn git_get_graph(
    path: String,
    limit: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GraphCommit>, ApiError> {
    Ok(state.git.get_graph(Path::new(&path), limit))
}
