use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::git::{CommitDetail, DiffContext, GitSummary, GraphCommit, StashEntry};
use std::path::{Path, PathBuf};

/// Reject anything that isn't a hex commit hash (40-char full or 7+ short).
pub(crate) fn validated_git_ref(hash: &str) -> Result<(), ApiError> {
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

// ── Write operations ────────────────────────────────────────────

/// Stage all changes (tracked + untracked).
#[tauri::command]
pub fn git_stage_all(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state
        .git
        .stage_all(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Unstage all staged changes.
#[tauri::command]
pub fn git_unstage_all(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state
        .git
        .unstage_all(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Discard all unstaged changes and untracked files.
#[tauri::command]
pub fn git_discard_all(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state
        .git
        .discard_all(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Create a commit with the given message.
#[tauri::command]
pub fn git_commit(
    path: String,
    message: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(ApiError::InvalidArgument(
            "Commit message cannot be empty".to_string(),
        ));
    }
    state
        .git
        .commit(Path::new(&path), trimmed)
        .map_err(ApiError::Internal)
}

/// Undo the last commit (soft reset to HEAD~1).
#[tauri::command]
pub fn git_undo_last_commit(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .undo_last_commit(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Push current branch to upstream.
#[tauri::command]
pub fn git_push(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.git.push(Path::new(&path)).map_err(ApiError::Internal)
}

/// Push with --force-with-lease.
#[tauri::command]
pub fn git_push_force_with_lease(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .push_force_with_lease(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Pull from upstream (fetch + merge).
#[tauri::command]
pub fn git_pull(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.git.pull(Path::new(&path)).map_err(ApiError::Internal)
}

/// Fetch from all remotes.
#[tauri::command]
pub fn git_fetch(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state
        .git
        .fetch(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Stash all changes including untracked files.
#[tauri::command]
pub fn git_stash_all(
    path: String,
    message: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .stash_all(Path::new(&path), message.as_deref())
        .map_err(ApiError::Internal)
}

/// Count stash entries (lightweight, no per-entry file stats).
#[tauri::command]
pub fn git_stash_count(path: String, state: tauri::State<'_, AppState>) -> Result<usize, ApiError> {
    state
        .git
        .stash_count(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// List all stash entries.
#[tauri::command]
pub fn git_stash_list(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<StashEntry>, ApiError> {
    Ok(state.git.stash_list(Path::new(&path)))
}

/// Pop a stash entry (apply + drop).
#[tauri::command]
pub fn git_stash_pop(
    path: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .stash_pop(Path::new(&path), index)
        .map_err(ApiError::Internal)
}

/// Drop a stash entry without applying.
#[tauri::command]
pub fn git_stash_drop(
    path: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .stash_drop(Path::new(&path), index)
        .map_err(ApiError::Internal)
}

/// Get all file diffs for a stash entry.
#[tauri::command]
pub fn git_get_stash_diffs(
    path: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<std::collections::HashMap<String, String>, ApiError> {
    Ok(state.git.get_stash_diffs(Path::new(&path), index))
}

/// Initialize a new git repository in the given directory.
#[tauri::command]
pub fn git_init(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.git.init(Path::new(&path)).map_err(ApiError::Internal)
}

/// Clone a repository into the given directory (in-place, no subfolder).
#[tauri::command]
pub fn git_clone_in_place(
    path: String,
    url: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .clone_in_place(Path::new(&path), &url)
        .map_err(ApiError::Internal)
}
