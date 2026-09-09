use crate::errors::ApiError;
use crate::host::Host;
use crate::services::git::{git_show_bounded, DeleteBranchError, ShowOutput};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use sworm_protocol::branch::{BranchOpState, BranchSummary};
use sworm_protocol::file_diff::{DiffSource, FileDiff, GitStatus};
use sworm_protocol::git::{
    CommitDetail, DiffFileContent, GitQuickDiffData, GitSummary, GraphCommit, StashEntry,
};

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

fn git_show_raw_text(repo: &Path, spec: &str) -> Option<String> {
    // Quick-diff bases feed hunk staging/reverting, so they must be the
    // actual blob text, not display-only textconv output.
    match git_show_bounded(repo, &["show", "--no-textconv", spec]) {
        ShowOutput::Bytes(bytes) => String::from_utf8(bytes).ok(),
        ShowOutput::Oversized | ShowOutput::Failed => None,
    }
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

/// Get git summary for a project path. Git status can scan the full tree, so
/// keep it off the async runtime thread.
impl Host {
    pub async fn git_get_summary(&self, path: String) -> Result<GitSummary, ApiError> {
        let git = std::sync::Arc::clone(&self.git);
        tokio::task::spawn_blocking(move || git.get_summary(Path::new(&path)))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
            .map_err(ApiError::Internal)
    }

    /// Watch Git metadata and the Git-aware working tree. Setup walks directories
    /// and invokes Git, so it also belongs on a blocking worker. Startup failures
    /// are returned; an unhealthy existing worker is replaced on a later call. A
    /// folder released while setup is in flight never keeps a watcher.
    pub async fn git_watch(
        &self,
        project_path: String,
        owned: impl FnOnce(&Path) -> bool + Send + 'static,
    ) -> Result<(), ApiError> {
        let watchers = std::sync::Arc::clone(&self.git_watchers);
        let events = std::sync::Arc::clone(&self.events);
        tokio::task::spawn_blocking(move || {
            let folder = Path::new(&project_path);
            watchers.watch(events, folder, || owned(folder))
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Internal)
    }

    /// Get full commit detail (metadata + file list with stats).
    pub async fn git_get_commit_detail(
        &self,
        path: String,
        hash: String,
    ) -> Result<Option<CommitDetail>, ApiError> {
        validated_git_ref(&hash)?;
        Ok(self.git.get_commit_detail(Path::new(&path), &hash))
    }

    /// Unified diff payload for the Monaco multi-file viewer. Returns one
    /// `FileDiff` per changed file, with both sides of content attached,
    /// regardless of whether the source is the working tree, a commit,
    /// or a stash. Replaces the mixed-shape `git_get_*_diffs` family.
    pub async fn diff_get_files(
        &self,
        path: String,
        source: DiffSource,
    ) -> Result<Vec<FileDiff>, ApiError> {
        // Validate refs up front so invalid input fails before we hit git.
        match &source {
            DiffSource::Commit { hash } => validated_git_ref(hash)?,
            DiffSource::Stash { .. } | DiffSource::Working { .. } => {}
        }
        Ok(self.git.get_diff_files(Path::new(&path), &source))
    }

    /// Cheap working-tree diff index: file list + metadata, no content.
    /// Pair with [`diff_get_working_file`] to load each file's content
    /// lazily; keeps the initial payload small even when the working
    /// tree has hundreds of changed files.
    pub async fn diff_get_working_index(
        &self,
        path: String,
        staged: bool,
    ) -> Result<Vec<FileDiff>, ApiError> {
        Ok(self.git.get_working_diff_index(Path::new(&path), staged))
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
    pub async fn diff_get_working_file(
        &self,
        path: String,
        file_path: String,
        status: GitStatus,
        staged: bool,
    ) -> Result<DiffFileContent, ApiError> {
        validated_project_file(&path, &file_path)?;
        let (old_content, new_content, binary) =
            self.git
                .get_working_diff_file_content(Path::new(&path), &file_path, status, staged);
        Ok(DiffFileContent {
            old_content,
            new_content,
            binary,
        })
    }

    /// Get commit graph data for visualization (all branches).
    pub async fn git_get_graph(
        &self,
        path: String,
        limit: usize,
    ) -> Result<Vec<GraphCommit>, ApiError> {
        Ok(self.git.get_graph(Path::new(&path), limit))
    }

    /// Get commit history reachable from one branch ref.
    pub async fn git_get_branch_commits(
        &self,
        path: String,
        branch: String,
        limit: usize,
    ) -> Result<Vec<GraphCommit>, ApiError> {
        validated_ref_name(&branch)?;
        Ok(self
            .git
            .get_branch_commits(Path::new(&path), &branch, limit))
    }

    // WRITE OPERATIONS //

    /// Stage all changes (tracked + untracked).
    pub async fn git_stage_all(&self, path: String) -> Result<(), ApiError> {
        self.git
            .stage_all(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Stage specific files or directories.
    pub async fn git_stage_files(&self, path: String, files: Vec<String>) -> Result<(), ApiError> {
        self.git
            .stage_files(Path::new(&path), &files)
            .map_err(ApiError::Internal)
    }

    /// Unstage all staged changes.
    pub async fn git_unstage_all(&self, path: String) -> Result<(), ApiError> {
        self.git
            .unstage_all(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Unstage specific files or directories.
    pub async fn git_unstage_files(
        &self,
        path: String,
        files: Vec<String>,
    ) -> Result<(), ApiError> {
        self.git
            .unstage_files(Path::new(&path), &files)
            .map_err(ApiError::Internal)
    }

    /// Discard all unstaged changes and untracked files.
    pub async fn git_discard_all(&self, path: String) -> Result<(), ApiError> {
        self.git
            .discard_all(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Discard changes for specific files or directories.
    pub async fn git_discard_files(
        &self,
        path: String,
        files: Vec<String>,
    ) -> Result<(), ApiError> {
        self.git
            .discard_files(Path::new(&path), &files)
            .map_err(ApiError::Internal)
    }

    /// Get the combined patch for all working-tree changes.
    pub async fn git_get_full_patch(&self, path: String) -> Result<Option<String>, ApiError> {
        Ok(self.git.get_full_patch(Path::new(&path)))
    }

    /// Get patch for specific paths, optionally scoped to staged or unstaged only.
    pub async fn git_get_path_patch(
        &self,
        path: String,
        files: Vec<String>,
        staged: Option<bool>,
    ) -> Result<Option<String>, ApiError> {
        Ok(self.git.get_path_patch(Path::new(&path), &files, staged))
    }

    /// Return Git bases used by the live editor dirty-diff gutter.
    pub async fn git_get_quick_diff_data(
        &self,
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
    pub async fn git_stage_file_content(
        &self,
        project_path: String,
        file_path: String,
        content: Option<String>,
    ) -> Result<(), ApiError> {
        validated_project_file(&project_path, &file_path)?;
        let repo = Path::new(&project_path);
        let result = match content {
            Some(content) => git_update_index_blob(repo, &file_path, &content),
            None => git_remove_index_path(repo, &file_path),
        };
        if result.is_ok() {
            self.git.invalidate(repo);
        }
        result
    }

    /// Create a commit with the given message.
    pub async fn git_commit(&self, path: String, message: String) -> Result<String, ApiError> {
        let trimmed = message.trim();
        if trimmed.is_empty() {
            return Err(ApiError::InvalidArgument(
                "Commit message cannot be empty".to_string(),
            ));
        }
        self.git
            .commit(Path::new(&path), trimmed)
            .map_err(ApiError::Internal)
    }

    /// Undo the last commit (soft reset to HEAD~1). Returns the commit
    /// message so the frontend can restore it into the commit textarea.
    pub async fn git_undo_last_commit(&self, path: String) -> Result<String, ApiError> {
        self.git
            .undo_last_commit(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Push current branch to upstream.
    pub async fn git_push(&self, path: String) -> Result<(), ApiError> {
        self.git.push(Path::new(&path)).map_err(ApiError::Internal)
    }

    /// Push with --force-with-lease.
    pub async fn git_push_force_with_lease(&self, path: String) -> Result<(), ApiError> {
        self.git
            .push_force_with_lease(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Pull from upstream (fetch + merge).
    pub async fn git_pull(&self, path: String) -> Result<(), ApiError> {
        self.git.pull(Path::new(&path)).map_err(ApiError::Internal)
    }

    /// Fetch from all remotes.
    pub async fn git_fetch(&self, path: String) -> Result<(), ApiError> {
        self.git.fetch(Path::new(&path)).map_err(ApiError::Internal)
    }

    /// Stash all changes including untracked files.
    pub async fn git_stash_all(
        &self,
        path: String,
        message: Option<String>,
    ) -> Result<(), ApiError> {
        self.git
            .stash_all(Path::new(&path), message.as_deref())
            .map_err(ApiError::Internal)
    }

    /// Count stash entries (lightweight, no per-entry file stats).
    pub async fn git_stash_count(&self, path: String) -> Result<usize, ApiError> {
        self.git
            .stash_count(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// List all stash entries.
    pub async fn git_stash_list(&self, path: String) -> Result<Vec<StashEntry>, ApiError> {
        Ok(self.git.stash_list(Path::new(&path)))
    }

    /// Pop a stash entry (apply + drop).
    pub async fn git_stash_pop(&self, path: String, index: usize) -> Result<(), ApiError> {
        self.git
            .stash_pop(Path::new(&path), index)
            .map_err(ApiError::Internal)
    }

    /// Drop a stash entry without applying.
    pub async fn git_stash_drop(&self, path: String, index: usize) -> Result<(), ApiError> {
        self.git
            .stash_drop(Path::new(&path), index)
            .map_err(ApiError::Internal)
    }

    /// Return file content at a specific git revision.
    /// Validates both the ref and file path before executing.
    /// Uses the raw blob (no textconv) capped at `MAX_CONTENT_BYTES`.
    pub async fn git_show_file(
        &self,
        project_path: String,
        git_ref: String,
        file_path: String,
    ) -> Result<String, ApiError> {
        validated_git_rev(&git_ref)?;
        validated_project_file(&project_path, &file_path)?;

        let repo = Path::new(&project_path);
        let rev_spec = format!("{}:{}", git_ref, file_path);
        git_show_raw_text(repo, &rev_spec).ok_or_else(|| {
            ApiError::NotFound(format!("Could not resolve {}:{}", git_ref, file_path))
        })
    }

    /// Initialize a new git repository in the given directory.
    pub async fn git_init(&self, path: String) -> Result<(), ApiError> {
        self.git.init(Path::new(&path)).map_err(ApiError::Internal)
    }

    /// Clone a repository into the given directory (in-place, no subfolder).
    pub async fn git_clone_in_place(&self, path: String, url: String) -> Result<(), ApiError> {
        self.git
            .clone_in_place(Path::new(&path), &url)
            .map_err(ApiError::Internal)
    }

    // BRANCH OPERATIONS //

    /// List every local + remote-tracking branch in one call.
    pub async fn git_list_branches(&self, path: String) -> Result<Vec<BranchSummary>, ApiError> {
        self.git
            .list_branches(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Current paused-state of the working tree (idle / rebasing / merging).
    pub async fn git_branch_status(&self, path: String) -> Result<BranchOpState, ApiError> {
        Ok(self.git.branch_status(Path::new(&path)))
    }

    /// File metadata for `branch...HEAD` compare.
    pub async fn git_diff_branch_against_head(
        &self,
        path: String,
        branch: String,
    ) -> Result<Vec<FileDiff>, ApiError> {
        validated_ref_name(&branch)?;
        Ok(self.git.diff_branch_against_head(Path::new(&path), &branch))
    }

    /// Switch HEAD to an existing branch.
    pub async fn git_checkout_branch(&self, path: String, name: String) -> Result<(), ApiError> {
        validated_ref_name(&name)?;
        let repo = Path::new(&path);
        if self.git.is_worktree_dirty(repo) {
            return Err(ApiError::DirtyWorktree {
                message: "Stash or commit changes before switching branches".to_string(),
            });
        }
        self.git
            .checkout_branch(repo, &name)
            .map_err(ApiError::Internal)
    }

    /// Create a tracking local branch from a remote ref and switch to it.
    pub async fn git_checkout_remote_as_local(
        &self,
        path: String,
        remote_name: String,
        local_name: String,
    ) -> Result<(), ApiError> {
        validated_ref_name(&remote_name)?;
        validated_ref_name(&local_name)?;
        let repo = Path::new(&path);
        if self.git.is_worktree_dirty(repo) {
            return Err(ApiError::DirtyWorktree {
                message: "Stash or commit changes before switching branches".to_string(),
            });
        }
        self.git
            .checkout_remote_as_local(repo, &remote_name, &local_name)
            .map_err(ApiError::Internal)
    }

    /// Create a branch off `base`, optionally switching to it after.
    pub async fn git_create_branch(
        &self,
        path: String,
        name: String,
        base: String,
        checkout: bool,
    ) -> Result<(), ApiError> {
        validated_ref_name(&name)?;
        validated_ref_name(&base)?;
        self.git
            .create_branch(Path::new(&path), &name, &base, checkout)
            .map_err(ApiError::Internal)
    }

    /// Rename a local branch.
    pub async fn git_rename_branch(
        &self,
        path: String,
        old_name: String,
        new_name: String,
    ) -> Result<(), ApiError> {
        validated_ref_name(&old_name)?;
        validated_ref_name(&new_name)?;
        self.git
            .rename_branch(Path::new(&path), &old_name, &new_name)
            .map_err(ApiError::Internal)
    }

    /// Delete a local branch. Without `force`, refuses unmerged branches
    /// with a typed error so the dialog can offer the force-delete fallback.
    pub async fn git_delete_branch(
        &self,
        path: String,
        name: String,
        force: bool,
    ) -> Result<(), ApiError> {
        validated_ref_name(&name)?;
        self.git
            .delete_branch(Path::new(&path), &name, force)
            .map_err(|err| match err {
                DeleteBranchError::BranchUnmerged { branch, message } => {
                    ApiError::BranchUnmerged { branch, message }
                }
                DeleteBranchError::Git(message) => ApiError::Internal(message),
            })
    }

    /// Delete a branch on a remote (`git push <remote> --delete <name>`).
    pub async fn git_delete_remote_branch(
        &self,
        path: String,
        remote: String,
        name: String,
    ) -> Result<(), ApiError> {
        validated_ref_name(&remote)?;
        validated_ref_name(&name)?;
        self.git
            .delete_remote_branch(Path::new(&path), &remote, &name)
            .map_err(ApiError::Internal)
    }

    /// Set or change a branch's upstream tracking ref.
    pub async fn git_set_upstream(
        &self,
        path: String,
        branch: String,
        upstream: String,
    ) -> Result<(), ApiError> {
        validated_ref_name(&branch)?;
        validated_upstream_ref(&upstream)?;
        self.git
            .set_upstream(Path::new(&path), &branch, &upstream)
            .map_err(ApiError::Internal)
    }

    /// Fast-forward a branch to its upstream without checking it out.
    /// Falls back to `git pull --ff-only` for the current branch.
    pub async fn git_fast_forward_branch(
        &self,
        path: String,
        name: String,
    ) -> Result<(), ApiError> {
        validated_ref_name(&name)?;
        self.git
            .fast_forward(Path::new(&path), &name)
            .map_err(ApiError::Internal)
    }

    /// Merge `source` into the current branch.
    pub async fn git_merge_into_current(
        &self,
        path: String,
        source: String,
        no_ff: bool,
    ) -> Result<(), ApiError> {
        validated_ref_name(&source)?;
        self.git
            .merge_into_current(Path::new(&path), &source, no_ff)
            .map_err(ApiError::Internal)
    }

    /// Rebase the current branch onto `target`.
    pub async fn git_rebase_current_onto(
        &self,
        path: String,
        target: String,
    ) -> Result<(), ApiError> {
        validated_ref_name(&target)?;
        self.git
            .rebase_current_onto(Path::new(&path), &target)
            .map_err(ApiError::Internal)
    }

    /// Continue a paused rebase after the user resolved conflicts.
    pub async fn git_rebase_continue(&self, path: String) -> Result<(), ApiError> {
        self.git
            .rebase_continue(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Skip the current commit during a paused rebase.
    pub async fn git_rebase_skip(&self, path: String) -> Result<(), ApiError> {
        self.git
            .rebase_skip(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Abort an in-flight rebase.
    pub async fn git_rebase_abort(&self, path: String) -> Result<(), ApiError> {
        self.git
            .rebase_abort(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Abort an in-flight merge.
    pub async fn git_merge_abort(&self, path: String) -> Result<(), ApiError> {
        self.git
            .merge_abort(Path::new(&path))
            .map_err(ApiError::Internal)
    }
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
