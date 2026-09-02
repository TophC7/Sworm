use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::branch::{BranchOpState, BranchSummary};
use crate::models::file_diff::{DiffSource, FileDiff};
use crate::services::git::{
    CommitDetail, DeleteBranchError, GitSummary, GraphCommit, StashEntry, MAX_CONTENT_BYTES,
};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Lightweight defense against arg-injection for branch / remote
/// names. Rejects empties, leading `-` (would be parsed as a flag by
/// the git CLI), embedded whitespace and control chars, the `..` / `@{`
/// sequences, and the metacharacters `git check-ref-format` rejects.
/// Frontend validation is more permissive (mirrors `check-ref-format`
/// directly); this layer only stops the obvious shell-side hazards.
pub(crate) fn validated_ref_name(name: &str) -> Result<(), ApiError> {
    if name.is_empty() {
        return Err(ApiError::InvalidArgument(
            "Branch name is empty".to_string(),
        ));
    }
    if name.starts_with('-') {
        return Err(ApiError::InvalidArgument(
            "Branch name cannot start with '-'".to_string(),
        ));
    }
    if name.contains("..") || name.contains("@{") {
        return Err(ApiError::InvalidArgument(format!(
            "Invalid branch name: {}",
            name
        )));
    }
    for c in name.chars() {
        if c.is_control() || c == ' ' || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\') {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid branch name: {}",
                name
            )));
        }
    }
    Ok(())
}

/// Validate an upstream tracking ref in `<remote>/<branch>` form.
pub(crate) fn validated_upstream_ref(upstream: &str) -> Result<(), ApiError> {
    let (remote, branch) = upstream.split_once('/').ok_or_else(|| {
        ApiError::InvalidArgument("Upstream must use <remote>/<branch>".to_string())
    })?;
    validated_ref_name(remote)?;
    validated_ref_name(branch)?;
    Ok(())
}

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
/// Returns `Ok(())` on success; callers pass the original paths to the
/// service layer (git CLI resolves them relative to its `current_dir`).
///
/// The candidate path may not exist on disk (deleted file, staged
/// addition, or a deleted file whose parent directory was also removed).
/// Walks up the candidate path until an ancestor canonicalizes
/// successfully, then re-attaches the missing suffix. This anchors path
/// resolution even when several leading segments are gone.
fn validated_project_file(project_path: &str, file_path: &str) -> Result<(), ApiError> {
    let root = PathBuf::from(project_path)
        .canonicalize()
        .map_err(|e| ApiError::InvalidArgument(format!("Invalid project path: {}", e)))?;

    let invalid = || ApiError::InvalidArgument("Invalid file path".to_string());
    let mut cursor = root.join(file_path);
    let mut suffix: Vec<std::ffi::OsString> = Vec::new();
    let normalized = loop {
        match cursor.canonicalize() {
            Ok(mut anchor) => {
                for segment in suffix.iter().rev() {
                    anchor.push(segment);
                }
                break anchor;
            }
            Err(_) => {
                suffix.push(cursor.file_name().ok_or_else(invalid)?.to_os_string());
                cursor = cursor.parent().ok_or_else(invalid)?.to_path_buf();
            }
        }
    };

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
pub async fn git_get_summary(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<GitSummary, ApiError> {
    Ok(state.git.get_summary(Path::new(&path)))
}

/// Get full commit detail (metadata + file list with stats).
#[tauri::command]
pub async fn git_get_commit_detail(
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
pub async fn diff_get_files(
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

/// Cheap working-tree diff index: file list + metadata, no content.
/// Pair with [`diff_get_working_file`] to load each file's content
/// lazily; keeps the initial payload small even when the working
/// tree has hundreds of changed files.
#[tauri::command]
pub async fn diff_get_working_index(
    path: String,
    staged: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileDiff>, ApiError> {
    Ok(state.git.get_working_diff_index(Path::new(&path), staged))
}

/// Working-tree per-file content. Returns the same shape as one
/// `FileDiff` entry from [`diff_get_files`], but with only the
/// requested file's content (rest of metadata fields populate from
/// what the caller already has via the index).
///
/// Validates `file_path` stays within `path` before touching disk so
/// a malicious or buggy frontend can't read arbitrary worktree files
/// via `../` traversal. Mirrors the guard on `git_get_quick_diff_data`,
/// `git_stage_file_content`, and `git_show_file`.
#[tauri::command]
pub async fn diff_get_working_file(
    path: String,
    file_path: String,
    status: crate::models::file_diff::GitStatus,
    staged: bool,
    state: tauri::State<'_, AppState>,
) -> Result<DiffFileContent, ApiError> {
    validated_project_file(&path, &file_path)?;
    let (old_content, new_content, binary) =
        state
            .git
            .get_working_diff_file_content(Path::new(&path), &file_path, status, staged);
    Ok(DiffFileContent {
        old_content,
        new_content,
        binary,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFileContent {
    pub old_content: Option<String>,
    pub new_content: Option<String>,
    pub binary: bool,
}

/// Get commit graph data for visualization (all branches).
#[tauri::command]
pub async fn git_get_graph(
    path: String,
    limit: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GraphCommit>, ApiError> {
    Ok(state.git.get_graph(Path::new(&path), limit))
}

/// Get commit history reachable from one branch ref.
#[tauri::command]
pub async fn git_get_branch_commits(
    path: String,
    branch: String,
    limit: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GraphCommit>, ApiError> {
    validated_ref_name(&branch)?;
    Ok(state
        .git
        .get_branch_commits(Path::new(&path), &branch, limit))
}

// WRITE OPERATIONS //

/// Stage all changes (tracked + untracked).
#[tauri::command]
pub async fn git_stage_all(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .stage_all(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Stage specific files or directories.
#[tauri::command]
pub async fn git_stage_files(
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
pub async fn git_unstage_all(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .unstage_all(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Unstage specific files or directories.
#[tauri::command]
pub async fn git_unstage_files(
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
pub async fn git_discard_all(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .discard_all(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Discard changes for specific files or directories.
#[tauri::command]
pub async fn git_discard_files(
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
pub async fn git_get_full_patch(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    Ok(state.git.get_full_patch(Path::new(&path)))
}

/// Get patch for specific paths, optionally scoped to staged or unstaged only.
#[tauri::command]
pub async fn git_get_path_patch(
    path: String,
    files: Vec<String>,
    staged: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    Ok(state.git.get_path_patch(Path::new(&path), &files, staged))
}

/// Return Git bases used by the live editor dirty-diff gutter.
#[tauri::command]
pub async fn git_get_quick_diff_data(
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
///
/// Invalidates the GitService summary cache on success so the very next
/// `git_get_summary` reflects the new index state. Without this the 300ms
/// TTL cache could serve a pre-stage summary to a frontend `refreshGit`
/// chained immediately after, hiding the stage from the UI.
#[tauri::command]
pub async fn git_stage_file_content(
    project_path: String,
    file_path: String,
    content: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_project_file(&project_path, &file_path)?;
    let repo = Path::new(&project_path);
    let result = match content {
        Some(content) => git_update_index_blob(repo, &file_path, &content),
        None => git_remove_index_path(repo, &file_path),
    };
    if result.is_ok() {
        state.git.invalidate(repo);
    }
    result
}

/// Create a commit with the given message.
#[tauri::command]
pub async fn git_commit(
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
pub async fn git_undo_last_commit(
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
pub async fn git_push(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.git.push(Path::new(&path)).map_err(ApiError::Internal)
}

/// Push with --force-with-lease.
#[tauri::command]
pub async fn git_push_force_with_lease(
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
pub async fn git_pull(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.git.pull(Path::new(&path)).map_err(ApiError::Internal)
}

/// Fetch from all remotes.
#[tauri::command]
pub async fn git_fetch(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state
        .git
        .fetch(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Stash all changes including untracked files.
#[tauri::command]
pub async fn git_stash_all(
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
pub async fn git_stash_count(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<usize, ApiError> {
    state
        .git
        .stash_count(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// List all stash entries.
#[tauri::command]
pub async fn git_stash_list(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<StashEntry>, ApiError> {
    Ok(state.git.stash_list(Path::new(&path)))
}

/// Pop a stash entry (apply + drop).
#[tauri::command]
pub async fn git_stash_pop(
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
pub async fn git_stash_drop(
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
/// Uses the raw blob (no textconv) capped at `MAX_CONTENT_BYTES`.
#[tauri::command]
pub async fn git_show_file(
    project_path: String,
    git_ref: String,
    file_path: String,
) -> Result<String, ApiError> {
    validated_git_rev(&git_ref)?;
    validated_project_file(&project_path, &file_path)?;

    let repo = Path::new(&project_path);
    let rev_spec = format!("{}:{}", git_ref, file_path);
    git_show_raw_text(repo, &rev_spec)
        .ok_or_else(|| ApiError::NotFound(format!("Could not resolve {}:{}", git_ref, file_path)))
}

/// Initialize a new git repository in the given directory.
#[tauri::command]
pub async fn git_init(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.git.init(Path::new(&path)).map_err(ApiError::Internal)
}

/// Clone a repository into the given directory (in-place, no subfolder).
#[tauri::command]
pub async fn git_clone_in_place(
    path: String,
    url: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .clone_in_place(Path::new(&path), &url)
        .map_err(ApiError::Internal)
}

// BRANCH OPERATIONS //

/// List every local + remote-tracking branch in one call.
#[tauri::command]
pub async fn git_list_branches(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BranchSummary>, ApiError> {
    state
        .git
        .list_branches(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Single-branch lookup. Used by the StatusBar popover and post-
/// mutation refresh path.
#[tauri::command]
pub async fn git_branch_info(
    path: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<BranchSummary, ApiError> {
    validated_ref_name(&name)?;
    state
        .git
        .branch_info(Path::new(&path), &name)
        .map_err(ApiError::NotFound)
}

/// Current paused-state of the working tree (idle / rebasing / merging).
#[tauri::command]
pub async fn git_branch_status(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<BranchOpState, ApiError> {
    Ok(state.git.branch_status(Path::new(&path)))
}

/// File metadata for `branch...HEAD` compare.
#[tauri::command]
pub async fn git_diff_branch_against_head(
    path: String,
    branch: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileDiff>, ApiError> {
    validated_ref_name(&branch)?;
    Ok(state
        .git
        .diff_branch_against_head(Path::new(&path), &branch))
}

/// Switch HEAD to an existing branch.
#[tauri::command]
pub async fn git_checkout_branch(
    path: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&name)?;
    let repo = Path::new(&path);
    if state.git.is_worktree_dirty(repo) {
        return Err(ApiError::DirtyWorktree {
            message: "Stash or commit changes before switching branches".to_string(),
        });
    }
    state
        .git
        .checkout_branch(repo, &name)
        .map_err(ApiError::Internal)
}

/// Create a tracking local branch from a remote ref and switch to it.
#[tauri::command]
pub async fn git_checkout_remote_as_local(
    path: String,
    remote_name: String,
    local_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&remote_name)?;
    validated_ref_name(&local_name)?;
    let repo = Path::new(&path);
    if state.git.is_worktree_dirty(repo) {
        return Err(ApiError::DirtyWorktree {
            message: "Stash or commit changes before switching branches".to_string(),
        });
    }
    state
        .git
        .checkout_remote_as_local(repo, &remote_name, &local_name)
        .map_err(ApiError::Internal)
}

/// Create a branch off `base`, optionally switching to it after.
#[tauri::command]
pub async fn git_create_branch(
    path: String,
    name: String,
    base: String,
    checkout: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&name)?;
    validated_ref_name(&base)?;
    state
        .git
        .create_branch(Path::new(&path), &name, &base, checkout)
        .map_err(ApiError::Internal)
}

/// Rename a local branch.
#[tauri::command]
pub async fn git_rename_branch(
    path: String,
    old_name: String,
    new_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&old_name)?;
    validated_ref_name(&new_name)?;
    state
        .git
        .rename_branch(Path::new(&path), &old_name, &new_name)
        .map_err(ApiError::Internal)
}

/// Delete a local branch. Without `force`, refuses unmerged branches
/// with a typed error so the dialog can offer the force-delete fallback.
#[tauri::command]
pub async fn git_delete_branch(
    path: String,
    name: String,
    force: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&name)?;
    state
        .git
        .delete_branch(Path::new(&path), &name, force)
        .map_err(|err| match err {
            DeleteBranchError::BranchUnmerged { branch, message } => {
                ApiError::BranchUnmerged { branch, message }
            }
            DeleteBranchError::Git(message) => ApiError::Internal(message),
        })
}

/// Delete a branch on a remote (`git push <remote> --delete <name>`).
#[tauri::command]
pub async fn git_delete_remote_branch(
    path: String,
    remote: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&remote)?;
    validated_ref_name(&name)?;
    state
        .git
        .delete_remote_branch(Path::new(&path), &remote, &name)
        .map_err(ApiError::Internal)
}

/// Set or change a branch's upstream tracking ref.
#[tauri::command]
pub async fn git_set_upstream(
    path: String,
    branch: String,
    upstream: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&branch)?;
    validated_upstream_ref(&upstream)?;
    state
        .git
        .set_upstream(Path::new(&path), &branch, &upstream)
        .map_err(ApiError::Internal)
}

/// Fast-forward a branch to its upstream without checking it out.
/// Falls back to `git pull --ff-only` for the current branch.
#[tauri::command]
pub async fn git_fast_forward_branch(
    path: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&name)?;
    state
        .git
        .fast_forward(Path::new(&path), &name)
        .map_err(ApiError::Internal)
}

/// Merge `source` into the current branch.
#[tauri::command]
pub async fn git_merge_into_current(
    path: String,
    source: String,
    no_ff: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&source)?;
    state
        .git
        .merge_into_current(Path::new(&path), &source, no_ff)
        .map_err(ApiError::Internal)
}

/// Rebase the current branch onto `target`.
#[tauri::command]
pub async fn git_rebase_current_onto(
    path: String,
    target: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    validated_ref_name(&target)?;
    state
        .git
        .rebase_current_onto(Path::new(&path), &target)
        .map_err(ApiError::Internal)
}

/// Continue a paused rebase after the user resolved conflicts.
#[tauri::command]
pub async fn git_rebase_continue(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .rebase_continue(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Skip the current commit during a paused rebase.
#[tauri::command]
pub async fn git_rebase_skip(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .rebase_skip(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Abort an in-flight rebase.
#[tauri::command]
pub async fn git_rebase_abort(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .rebase_abort(Path::new(&path))
        .map_err(ApiError::Internal)
}

/// Abort an in-flight merge.
#[tauri::command]
pub async fn git_merge_abort(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .git
        .merge_abort(Path::new(&path))
        .map_err(ApiError::Internal)
}

#[cfg(test)]
mod tests {
    #[test]
    fn validated_upstream_ref_accepts_remote_branch_shape() {
        use super::validated_upstream_ref;

        assert!(validated_upstream_ref("origin/main").is_ok());
        assert!(validated_upstream_ref("origin/feature/nested").is_ok());
    }

    #[test]
    fn validated_upstream_ref_rejects_missing_side_or_flag_shape() {
        use super::validated_upstream_ref;

        assert!(validated_upstream_ref("main").is_err());
        assert!(validated_upstream_ref("/main").is_err());
        assert!(validated_upstream_ref("origin/").is_err());
        assert!(validated_upstream_ref("-origin/main").is_err());
    }

    #[test]
    fn validated_project_file_rejects_traversal() {
        use super::validated_project_file;
        // /tmp is canonicalizable on every supported host. /etc/passwd
        // is unrelated to /tmp so the traversal escape is detected.
        let project = "/tmp";
        assert!(validated_project_file(project, "ok.txt").is_ok());
        assert!(validated_project_file(project, "../etc/passwd").is_err());
        assert!(validated_project_file(project, "../../../../etc/passwd").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn validated_project_file_accepts_deleted_paths() {
        use super::validated_project_file;
        use std::fs;

        let project = std::env::temp_dir().join(format!(
            "sworm-validate-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&project).expect("create temp project");
        let project_str = project.to_string_lossy().into_owned();

        // File deleted, parent intact.
        assert!(validated_project_file(&project_str, "missing.txt").is_ok());

        // File and parent directory both missing; walk-up anchors to the project root.
        assert!(validated_project_file(&project_str, "vanished/dir/file.txt").is_ok());

        // Traversal still rejected even when the candidate doesn't exist.
        assert!(validated_project_file(&project_str, "../etc/passwd").is_err());

        fs::remove_dir_all(&project).expect("remove temp project");
    }

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
