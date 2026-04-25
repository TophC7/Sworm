use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::file_diff::{DiffSource, FileDiff};
use crate::services::git::{CommitDetail, GitSummary, GraphCommit, StashEntry, MAX_CONTENT_BYTES};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Accept hex commit hashes OR `stash@{N}` references.
pub(crate) fn validated_git_rev(rev: &str) -> Result<(), ApiError> {
    // Hex commit hash (7–40 chars)
    if rev.len() >= 7 && rev.len() <= 40 && rev.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(());
    }
    // stash@{N} format
    if let Some(inner) = rev
        .strip_prefix("stash@{")
        .and_then(|s| s.strip_suffix('}'))
    {
        if !inner.is_empty() && inner.bytes().all(|b| b.is_ascii_digit()) {
            return Ok(());
        }
    }
    Err(ApiError::InvalidArgument(format!(
        "Invalid git revision: {}",
        rev
    )))
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
            // Path may not exist yet (e.g. staging a new file). Canonicalize
            // the parent and re-attach the file name; reject if the path has
            // no concrete final segment to anchor the parent walk.
            let parent = candidate.parent().unwrap_or(&candidate).canonicalize()?;
            let name = candidate.file_name().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Missing file name")
            })?;
            Ok::<PathBuf, std::io::Error>(parent.join(name))
        })
        .map_err(|e| ApiError::InvalidArgument(format!("Invalid file path: {}", e)))?;

    if !normalized.starts_with(&root) {
        return Err(ApiError::InvalidArgument(
            "File path must stay within the project root".to_string(),
        ));
    }

    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitQuickDiffData {
    pub index_content: Option<String>,
    pub head_content: Option<String>,
    pub has_index_changes: bool,
}

fn git_show_raw_text(repo: &Path, spec: &str) -> Option<String> {
    let output = Command::new("git")
        // Quick-diff bases feed hunk staging/reverting, so they must be the
        // actual blob text, not display-only textconv output.
        .args(["--no-optional-locks", "show", "--no-textconv", spec])
        .current_dir(repo)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }
    if output.stdout.len() > MAX_CONTENT_BYTES {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}

fn git_file_mode(repo: &Path, file_path: &str) -> Result<String, ApiError> {
    let index = Command::new("git")
        .args(["--no-optional-locks", "ls-files", "-s", "--"])
        .arg(file_path)
        .current_dir(repo)
        .output()
        .map_err(|e| ApiError::Internal(format!("Failed to read git index: {}", e)))?;

    if index.status.success() {
        let line = String::from_utf8_lossy(&index.stdout);
        if let Some(mode) = line.split_whitespace().next() {
            if !mode.is_empty() {
                return Ok(mode.to_string());
            }
        }
    }

    let head = Command::new("git")
        .args(["--no-optional-locks", "ls-tree", "HEAD", "--"])
        .arg(file_path)
        .current_dir(repo)
        .output()
        .map_err(|e| ApiError::Internal(format!("Failed to read HEAD tree: {}", e)))?;

    if head.status.success() {
        let line = String::from_utf8_lossy(&head.stdout);
        if let Some(mode) = line.split_whitespace().next() {
            if !mode.is_empty() {
                return Ok(mode.to_string());
            }
        }
    }

    if let Some(mode) = git_worktree_file_mode(repo, file_path)? {
        return Ok(mode);
    }

    Ok("100644".to_string())
}

#[cfg(unix)]
fn git_worktree_file_mode(repo: &Path, file_path: &str) -> Result<Option<String>, ApiError> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = match std::fs::symlink_metadata(repo.join(file_path)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ApiError::Internal(format!(
                "Failed to read worktree file mode: {}",
                error
            )));
        }
    };

    if metadata.file_type().is_symlink() {
        return Ok(Some("120000".to_string()));
    }

    if metadata.permissions().mode() & 0o111 != 0 {
        return Ok(Some("100755".to_string()));
    }

    Ok(Some("100644".to_string()))
}

#[cfg(not(unix))]
fn git_worktree_file_mode(_repo: &Path, _file_path: &str) -> Result<Option<String>, ApiError> {
    Ok(None)
}

fn git_hash_object(repo: &Path, file_path: &str, content: &str) -> Result<String, ApiError> {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin", "-w", "--path"])
        .arg(file_path)
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ApiError::Internal(format!("Failed to spawn git hash-object: {}", e)))?;

    let stdin = child
        .stdin
        .as_mut()
        .ok_or_else(|| ApiError::Internal("Failed to open git hash-object stdin".to_string()))?;
    stdin
        .write_all(content.as_bytes())
        .map_err(|e| ApiError::Internal(format!("Failed to write git object content: {}", e)))?;
    let _ = child.stdin.take();

    let output = child
        .wait_with_output()
        .map_err(|e| ApiError::Internal(format!("Failed to finish git hash-object: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ApiError::Internal(format!(
            "git hash-object failed{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {}", stderr)
            }
        )));
    }

    let hash = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if hash.is_empty() {
        return Err(ApiError::Internal(
            "git hash-object returned an empty object id".to_string(),
        ));
    }

    Ok(hash)
}

fn git_update_index_blob(repo: &Path, file_path: &str, content: &str) -> Result<(), ApiError> {
    let mode = git_file_mode(repo, file_path)?;
    let hash = git_hash_object(repo, file_path, content)?;
    let output = Command::new("git")
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(mode)
        .arg(hash)
        .arg(file_path)
        .current_dir(repo)
        .output()
        .map_err(|e| ApiError::Internal(format!("Failed to update git index: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ApiError::Internal(format!(
            "git update-index failed{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {}", stderr)
            }
        )));
    }

    Ok(())
}

fn git_remove_index_path(repo: &Path, file_path: &str) -> Result<(), ApiError> {
    let output = Command::new("git")
        .args(["update-index", "--force-remove", "--"])
        .arg(file_path)
        .current_dir(repo)
        .output()
        .map_err(|e| ApiError::Internal(format!("Failed to remove path from git index: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(ApiError::Internal(format!(
            "git update-index --force-remove failed{}",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {}", stderr)
            }
        )));
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

/// Unified diff payload for the Monaco multi-file viewer. Returns one
/// `FileDiff` per changed file, with both sides of content attached,
/// regardless of whether the source is the working tree, a commit,
/// or a stash. Replaces the mixed-shape `git_get_*_diffs` family.
#[tauri::command]
pub fn diff_get_files(
    path: String,
    source: DiffSource,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileDiff>, ApiError> {
    // Validate refs up front so invalid input fails before we hit git.
    match &source {
        DiffSource::Commit { hash } => validated_git_ref(hash)?,
        DiffSource::Stash { .. } | DiffSource::Working { .. } => {}
    }
    Ok(state.git.get_diff_files(Path::new(&path), &source))
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

/// Stage specific files or directories.
#[tauri::command]
pub fn git_stage_files(
    path: String,
    files: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .stage_files(Path::new(&path), &files)
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

/// Unstage specific files or directories.
#[tauri::command]
pub fn git_unstage_files(
    path: String,
    files: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .unstage_files(Path::new(&path), &files)
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

/// Discard changes for specific files or directories.
#[tauri::command]
pub fn git_discard_files(
    path: String,
    files: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .discard_files(Path::new(&path), &files)
        .map_err(ApiError::Internal)
}

/// Get the combined patch for all working-tree changes.
#[tauri::command]
pub fn git_get_full_patch(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    Ok(state.git.get_full_patch(Path::new(&path)))
}

/// Get patch for specific paths, optionally scoped to staged or unstaged only.
#[tauri::command]
pub fn git_get_path_patch(
    path: String,
    files: Vec<String>,
    staged: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    Ok(state.git.get_path_patch(Path::new(&path), &files, staged))
}

/// Return Git bases used by the live editor dirty-diff gutter.
#[tauri::command]
pub fn git_get_quick_diff_data(
    project_path: String,
    file_path: String,
) -> Result<GitQuickDiffData, ApiError> {
    validated_project_file(&project_path, &file_path)?;

    let repo = Path::new(&project_path);
    let index_spec = format!(":{}", file_path);
    let head_spec = format!("HEAD:{}", file_path);
    let index_content = git_show_raw_text(repo, &index_spec);
    let head_content = git_show_raw_text(repo, &head_spec);
    // Derive instead of spawning `git diff --cached --quiet`. Mode-only
    // changes aren't reflected, but the dirty-diff editor is text-only.
    let has_index_changes = index_content != head_content;
    Ok(GitQuickDiffData {
        index_content,
        head_content,
        has_index_changes,
    })
}

/// Replace a single path's index blob with caller-supplied text content.
/// This mirrors VS Code's hunk staging strategy: synthesize the desired
/// index file content on the frontend, then update only Git's index here.
#[tauri::command]
pub fn git_stage_file_content(
    project_path: String,
    file_path: String,
    content: Option<String>,
) -> Result<(), ApiError> {
    validated_project_file(&project_path, &file_path)?;
    match content {
        Some(content) => git_update_index_blob(Path::new(&project_path), &file_path, &content),
        None => git_remove_index_path(Path::new(&project_path), &file_path),
    }
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

/// Undo the last commit (soft reset to HEAD~1). Returns the commit
/// message so the frontend can restore it into the commit textarea.
#[tauri::command]
pub fn git_undo_last_commit(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
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

/// Return file content at a specific git revision.
/// Validates both the ref and file path before executing.
#[tauri::command]
pub fn git_show_file(
    project_path: String,
    git_ref: String,
    file_path: String,
) -> Result<String, ApiError> {
    validated_git_rev(&git_ref)?;
    validated_project_file(&project_path, &file_path)?;

    let repo = Path::new(&project_path);
    let rev_spec = format!("{}:{}", git_ref, file_path);
    super::fresh::git_show(repo, &rev_spec)
        .ok_or_else(|| ApiError::NotFound(format!("Could not resolve {}:{}", git_ref, file_path)))
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

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn worktree_file_mode_preserves_executable_bit() {
        use super::git_worktree_file_mode;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let repo = std::env::temp_dir().join(format!(
            "sworm-git-mode-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&repo).expect("create temp repo dir");

        let script = repo.join("script.sh");
        fs::write(&script, "#!/bin/sh\n").expect("write script");
        let mut permissions = fs::metadata(&script)
            .expect("read script metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).expect("mark script executable");

        let mode = git_worktree_file_mode(&repo, "script.sh").expect("read worktree mode");
        fs::remove_dir_all(&repo).expect("remove temp repo dir");

        assert_eq!(mode.as_deref(), Some("100755"));
    }
}
