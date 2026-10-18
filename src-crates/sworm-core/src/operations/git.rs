use crate::errors::ApiError;
use crate::events::HostEvent;
use crate::host::Host;
use crate::services::git::validated_ref_name;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use sworm_protocol::branch::BranchSummary;
use sworm_protocol::file_diff::{DiffSource, FileDiff, GitStatus};
use sworm_protocol::git::{
    CommitDetail, DiffFileContent, GitBranchOp, GitBrief, GitChangeScope, GitChangedEvent,
    GitCommitOptions, GitConflictsOp, GitHistoryOp, GitOperationKind, GitPathsOp, GitQuickDiffData,
    GitRemote, GitRemoteConfigOp, GitRemoteOp, GitSequencerAction, GitStashOp, GitSummary, GitTag,
    GitTagOp, GitWorktreeCheckout, GitWorktreeList, GitWorktreeOp, GitWorktreeResult, GraphCommit,
    StashEntry,
};

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

pub(crate) fn validated_commitish(rev: &str) -> Result<(), ApiError> {
    validated_git_ref(rev).or_else(|_| validated_ref_name(rev))
}

fn validated_remote_url(url: &str) -> Result<(), ApiError> {
    if url.is_empty() || url.starts_with('-') || url.chars().any(char::is_control) {
        return Err(ApiError::InvalidArgument("Invalid remote URL".to_string()));
    }
    Ok(())
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

/// Get git summary for a project path. Git status can scan the full tree, so
/// keep it off the async runtime thread.
impl Host {
    pub fn git_get_summary(&self, path: String) -> Result<GitSummary, ApiError> {
        self.git
            .get_summary(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Branch, changed-path count and upstream ahead/behind from one `git status`.
    pub fn git_get_brief(&self, path: String) -> Result<GitBrief, ApiError> {
        self.git
            .get_brief(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Watch Git metadata and the Git-aware working tree. Setup walks directories
    /// and invokes Git, so it also belongs on a blocking worker. Startup failures
    /// are returned; an unhealthy existing worker is replaced on a later call. A
    /// folder released while setup is in flight never keeps a watcher.
    pub fn git_watch(
        &self,
        project_path: String,
        owned: impl FnOnce(&Path) -> bool + Send + 'static,
    ) -> Result<(), ApiError> {
        let folder = Path::new(&project_path);
        self.git_watchers
            .watch(std::sync::Arc::clone(&self.events), folder, || {
                owned(folder)
            })
            .map_err(ApiError::Internal)
    }

    /// Get full commit detail (metadata + file list with stats).
    pub fn git_get_commit_detail(
        &self,
        path: String,
        hash: String,
    ) -> Result<Option<CommitDetail>, ApiError> {
        validated_git_ref(&hash)?;
        Ok(self.git.get_commit_detail(Path::new(&path), &hash))
    }

    /// Cheap working-tree diff index: file list + metadata, no content.
    /// Pair with [`Self::diff_get_file`] to load each file's content
    /// lazily; keeps the initial payload small even when the working
    /// tree has hundreds of changed files.
    pub fn diff_get_working_index(
        &self,
        path: String,
        staged: bool,
    ) -> Result<Vec<FileDiff>, ApiError> {
        Ok(self.git.get_working_diff_index(Path::new(&path), staged))
    }

    /// Load both sides of one working-tree, commit, stash, or range diff.
    ///
    /// Validates `file_path` and `old_path` stay within `path` before touching
    /// disk so a malicious or buggy frontend can't read arbitrary worktree
    /// files via `../` traversal. Mirrors the guard on `git_get_quick_diff_data`,
    /// `git_stage_file_content`, and `git_show_file`.
    pub fn diff_get_file(
        &self,
        path: String,
        source: DiffSource,
        file_path: String,
        old_path: Option<String>,
        status: GitStatus,
    ) -> Result<DiffFileContent, ApiError> {
        match &source {
            DiffSource::Commit { hash } => validated_git_ref(hash)?,
            DiffSource::Range { base, target } => {
                validated_git_ref(base)?;
                if let Some(target) = target {
                    validated_git_ref(target)?;
                }
            }
            DiffSource::Working { .. } | DiffSource::Stash { .. } => {}
        }
        validated_project_file(&path, &file_path)?;
        if let Some(old) = &old_path {
            validated_project_file(&path, old)?;
        }
        let (old_content, new_content, binary) = self.git.get_diff_file_content(
            Path::new(&path),
            &source,
            &file_path,
            old_path.as_deref(),
            status,
        );
        Ok(DiffFileContent {
            old_content,
            new_content,
            binary,
        })
    }

    /// Get commit graph data for visualization (all branches).
    pub fn git_get_graph(&self, path: String, limit: usize) -> Result<Vec<GraphCommit>, ApiError> {
        Ok(self.git.get_graph(Path::new(&path), limit))
    }

    /// Get commit history reachable from one branch ref.
    pub fn git_get_branch_commits(
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

    /// Stage, unstage, discard, or ignore working-tree paths.
    pub fn git_paths(&self, path: String, op: GitPathsOp) -> Result<(), ApiError> {
        let repo = Path::new(&path);
        match op {
            GitPathsOp::Stage {
                files,
                allow_conflict_markers,
            } => self
                .git
                .stage(repo, files.as_deref(), allow_conflict_markers),
            GitPathsOp::Unstage { files } => self.git.unstage(repo, files.as_deref()),
            GitPathsOp::Discard { files, permanent } => {
                self.git.discard(repo, files.as_deref(), permanent)
            }
            GitPathsOp::Ignore { files } => {
                for file in &files {
                    validated_project_file(&path, file)?;
                }
                self.git.ignore(repo, &files)
            }
        }
    }

    /// Get the combined patch for all working-tree changes.
    pub fn git_get_full_patch(&self, path: String) -> Result<Option<String>, ApiError> {
        Ok(self.git.get_full_patch(Path::new(&path)))
    }

    /// Get patch for specific paths, optionally scoped to staged or unstaged only.
    pub fn git_get_path_patch(
        &self,
        path: String,
        files: Vec<String>,
        staged: Option<bool>,
    ) -> Result<Option<String>, ApiError> {
        Ok(self.git.get_path_patch(Path::new(&path), &files, staged))
    }

    /// Return Git bases used by the live editor dirty-diff gutter.
    pub fn git_get_quick_diff_data(
        &self,
        project_path: String,
        file_path: String,
    ) -> Result<GitQuickDiffData, ApiError> {
        validated_project_file(&project_path, &file_path)?;

        Ok(self
            .git
            .quick_diff_data(Path::new(&project_path), &file_path))
    }

    /// Replace a single path's index blob with caller-supplied text content.
    /// This mirrors VS Code's hunk staging strategy: synthesize the desired
    /// index file content on the frontend, then update only Git's index here.
    ///
    /// Invalidates the GitService summary cache on success so the very next
    /// `git_get_summary` reflects the new index state. Without this the 300ms
    /// TTL cache could serve a pre-stage summary to a frontend `refreshGit`
    /// chained immediately after, hiding the stage from the UI.
    pub fn git_stage_file_content(
        &self,
        project_path: String,
        file_path: String,
        content: Option<String>,
    ) -> Result<(), ApiError> {
        validated_project_file(&project_path, &file_path)?;
        self.git
            .stage_file_content(Path::new(&project_path), &file_path, content.as_deref())
    }

    /// Create or amend a commit with the given message and options.
    pub fn git_commit(
        &self,
        path: String,
        message: String,
        options: GitCommitOptions,
    ) -> Result<String, ApiError> {
        let trimmed = message.trim();
        if trimmed.is_empty() && !options.amend {
            return Err(ApiError::InvalidArgument(
                "Commit message cannot be empty".to_string(),
            ));
        }
        self.git.commit(Path::new(&path), trimmed, &options)
    }

    /// Undo the last commit, keeping its changes staged (an unborn branch for
    /// a root commit). Returns the message for the commit textarea.
    pub fn git_undo_last_commit(&self, path: String) -> Result<String, ApiError> {
        self.git.undo_last_commit(Path::new(&path))
    }

    /// Fetch, pull, push, publish, or synchronize with a remote.
    pub fn git_remote(&self, path: String, op: GitRemoteOp) -> Result<(), ApiError> {
        let repo = Path::new(&path);
        match op {
            GitRemoteOp::Fetch { remote } => {
                if let Some(remote) = &remote {
                    validated_ref_name(remote)?;
                }
                self.git.fetch(repo, remote.as_deref())
            }
            GitRemoteOp::Pull { rebase } => self.git.pull(repo, rebase),
            GitRemoteOp::Push { branch, force } => {
                if let Some(branch) = &branch {
                    validated_ref_name(branch)?;
                }
                self.git.push(repo, branch.as_deref(), force)
            }
            GitRemoteOp::Publish { branch, remote } => {
                if let Some(branch) = &branch {
                    validated_ref_name(branch)?;
                }
                if let Some(remote) = &remote {
                    validated_ref_name(remote)?;
                }
                self.git.publish(repo, branch.as_deref(), remote.as_deref())
            }
            GitRemoteOp::Sync { rebase } => self.git.sync(repo, rebase),
        }
    }

    /// Create, apply, remove, or branch from stash entries.
    pub fn git_stash(&self, path: String, op: GitStashOp) -> Result<(), ApiError> {
        let repo = Path::new(&path);
        match op {
            GitStashOp::Push { message, scope } => {
                self.git.stash_push(repo, message.as_deref(), &scope)
            }
            GitStashOp::Apply { index } => self.git.stash_apply(repo, index, false),
            GitStashOp::Pop { index } => self.git.stash_apply(repo, index, true),
            GitStashOp::Drop { index } => self.git.stash_drop(repo, index),
            GitStashOp::Clear => self.git.stash_clear(repo),
            GitStashOp::Branch { index, name } => {
                validated_ref_name(&name)?;
                self.git.stash_branch(repo, index, &name)
            }
        }
    }

    /// Count stash entries (lightweight, no per-entry file stats).
    pub fn git_stash_count(&self, path: String) -> Result<usize, ApiError> {
        self.git
            .stash_count(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// List all stash entries.
    pub fn git_stash_list(&self, path: String) -> Result<Vec<StashEntry>, ApiError> {
        Ok(self.git.stash_list(Path::new(&path)))
    }

    /// Return file content at a specific git revision.
    /// Validates both the ref and file path before executing.
    /// Uses the raw blob (no textconv), subject to the service content cap.
    pub fn git_show_file(
        &self,
        project_path: String,
        git_ref: String,
        file_path: String,
    ) -> Result<String, ApiError> {
        validated_git_rev(&git_ref)?;
        validated_project_file(&project_path, &file_path)?;

        self.git
            .show_file(Path::new(&project_path), &git_ref, &file_path)
            .ok_or_else(|| {
                ApiError::NotFound(format!("Could not resolve {}:{}", git_ref, file_path))
            })
    }

    /// Initialize a new git repository in the given directory.
    pub fn git_init(&self, path: String) -> Result<(), ApiError> {
        self.git.init(Path::new(&path))
    }

    /// Clone a repository into the given directory (in-place, no subfolder).
    pub fn git_clone_in_place(&self, path: String, url: String) -> Result<(), ApiError> {
        self.git.clone_in_place(Path::new(&path), &url)
    }

    // BRANCH OPERATIONS //

    /// List every local + remote-tracking branch in one call.
    pub fn git_list_branches(&self, path: String) -> Result<Vec<BranchSummary>, ApiError> {
        self.git
            .list_branches(Path::new(&path))
            .map_err(ApiError::Internal)
    }

    /// Configured remotes with fetch/push URLs.
    pub fn git_list_remotes(&self, path: String) -> Result<Vec<GitRemote>, ApiError> {
        self.git.list_remotes(Path::new(&path))
    }

    pub fn git_list_tags(&self, path: String) -> Result<Vec<GitTag>, ApiError> {
        self.git.list_tags(Path::new(&path))
    }

    pub fn git_tag(&self, path: String, op: GitTagOp) -> Result<(), ApiError> {
        match &op {
            GitTagOp::Create { name, rev, .. } => {
                validated_ref_name(name)?;
                validated_commitish(rev)?;
            }
            GitTagOp::Delete { name } => validated_ref_name(name)?,
            GitTagOp::Push { remote, name } => {
                validated_ref_name(remote)?;
                if let Some(name) = name {
                    validated_ref_name(name)?;
                }
            }
            GitTagOp::DeleteRemote { remote, name } => {
                validated_ref_name(remote)?;
                validated_ref_name(name)?;
            }
        }
        self.git.tag(Path::new(&path), op)
    }

    pub fn git_remote_config(&self, path: String, op: GitRemoteConfigOp) -> Result<(), ApiError> {
        match &op {
            GitRemoteConfigOp::Add { name, url } | GitRemoteConfigOp::SetUrl { name, url } => {
                validated_ref_name(name)?;
                validated_remote_url(url)?;
            }
            GitRemoteConfigOp::Remove { name } => validated_ref_name(name)?,
            GitRemoteConfigOp::Rename { old_name, new_name } => {
                validated_ref_name(old_name)?;
                validated_ref_name(new_name)?;
            }
        }
        self.git.remote_config(Path::new(&path), op)
    }

    pub fn git_history(&self, path: String, op: GitHistoryOp) -> Result<(), ApiError> {
        match &op {
            GitHistoryOp::CherryPick { rev }
            | GitHistoryOp::Revert { rev }
            | GitHistoryOp::Reset { rev, .. } => validated_commitish(rev)?,
        }
        self.git.history(Path::new(&path), op)
    }

    pub fn git_diff_range(
        &self,
        path: String,
        base: String,
        target: Option<String>,
    ) -> Result<Vec<FileDiff>, ApiError> {
        validated_git_ref(&base)?;
        if let Some(target) = &target {
            validated_git_ref(target)?;
        }
        Ok(self
            .git
            .diff_range(Path::new(&path), &base, target.as_deref()))
    }

    pub fn git_merged_branches(&self, path: String) -> Result<Vec<String>, ApiError> {
        self.git.merged_branches(Path::new(&path))
    }

    /// Switch branches or update their refs and tracking configuration.
    pub fn git_branch(&self, path: String, op: GitBranchOp) -> Result<(), ApiError> {
        let repo = Path::new(&path);
        match &op {
            GitBranchOp::Checkout { name } => validated_ref_name(name)?,
            GitBranchOp::CheckoutDetached { rev } => validated_commitish(rev)?,
            GitBranchOp::CheckoutRemote {
                remote_name,
                local_name,
            } => {
                validated_ref_name(remote_name)?;
                validated_ref_name(local_name)?;
            }
            _ => {}
        }
        if matches!(
            &op,
            GitBranchOp::Checkout { .. }
                | GitBranchOp::CheckoutDetached { .. }
                | GitBranchOp::CheckoutRemote { .. }
        ) && self.git.is_worktree_dirty(repo)
        {
            return Err(ApiError::DirtyWorktree {
                message: "Stash or commit changes before switching branches".to_string(),
            });
        }
        match op {
            GitBranchOp::Checkout { name } => self.git.checkout_branch(repo, &name),
            GitBranchOp::CheckoutDetached { rev } => self.git.checkout_detached(repo, &rev),
            GitBranchOp::CheckoutRemote {
                remote_name,
                local_name,
            } => self
                .git
                .checkout_remote_as_local(repo, &remote_name, &local_name),
            GitBranchOp::Create {
                name,
                base,
                checkout,
            } => {
                validated_ref_name(&name)?;
                validated_ref_name(&base)?;
                self.git.create_branch(repo, &name, &base, checkout)
            }
            GitBranchOp::Rename { old_name, new_name } => {
                validated_ref_name(&old_name)?;
                validated_ref_name(&new_name)?;
                self.git.rename_branch(repo, &old_name, &new_name)
            }
            GitBranchOp::Delete { name, force } => {
                validated_ref_name(&name)?;
                self.git.delete_branch(repo, &name, force)
            }
            GitBranchOp::DeleteRemote { remote, name } => {
                validated_ref_name(&remote)?;
                validated_ref_name(&name)?;
                self.git.delete_remote_branch(repo, &remote, &name)
            }
            GitBranchOp::SetUpstream { branch, upstream } => {
                validated_ref_name(&branch)?;
                validated_upstream_ref(&upstream)?;
                self.git.set_upstream(repo, &branch, &upstream)
            }
            GitBranchOp::UnsetUpstream { branch } => {
                validated_ref_name(&branch)?;
                self.git.unset_upstream(repo, &branch)
            }
            GitBranchOp::FastForward { name } => {
                validated_ref_name(&name)?;
                self.git.fast_forward(repo, &name)
            }
            GitBranchOp::Merge { source, no_ff } => {
                validated_ref_name(&source)?;
                self.git.merge_into_current(repo, &source, no_ff)
            }
            GitBranchOp::Rebase { target } => {
                validated_ref_name(&target)?;
                self.git.rebase_current_onto(repo, &target)
            }
        }
    }

    /// Continue, skip, or abort a paused Git operation.
    pub fn git_sequencer(
        &self,
        path: String,
        operation: GitOperationKind,
        action: GitSequencerAction,
    ) -> Result<(), ApiError> {
        self.git.sequencer(Path::new(&path), operation, &action)
    }

    /// Resolve conflicted paths by taking a side or deleting them.
    pub fn git_conflicts(&self, path: String, op: GitConflictsOp) -> Result<(), ApiError> {
        let repo = Path::new(&path);
        match op {
            GitConflictsOp::TakeSide { files, side } => {
                self.git.take_conflict_side(repo, &files, side)
            }
            GitConflictsOp::Delete { files } => self.git.delete_conflicted(repo, &files),
        }
    }
    pub fn git_worktree_list(&self, path: &str) -> Result<GitWorktreeList, ApiError> {
        self.git.list_worktrees(Path::new(path))
    }

    pub fn git_worktree_default_path(
        &self,
        path: &str,
        checkout: GitWorktreeCheckout,
        root: Option<String>,
    ) -> Result<String, ApiError> {
        let common = self.git.worktree_common_dir(Path::new(path))?;
        let root = match root {
            Some(root) => Some(root),
            None => self
                .app_state_get(format!("git:worktree-root:{common}"))?
                .map(|value| {
                    serde_json::from_str::<String>(&value).map_err(|error| {
                        ApiError::Internal(format!("Invalid remembered worktree root: {error}"))
                    })
                })
                .transpose()?,
        };
        self.git
            .default_worktree_path(Path::new(path), &checkout, root.as_deref())
    }

    /// Fresh membership/roots for the transport's ownership reservation and
    /// authoritative drain. Mechanics validate again under the common-dir lock.
    pub fn git_worktree_affected_roots(
        &self,
        path: &str,
        op: &GitWorktreeOp,
    ) -> Result<Vec<String>, ApiError> {
        self.git.worktree_affected_roots(Path::new(path), op)
    }

    /// Remove/Migrate may only be invoked by the custom transport path while its
    /// exclusive admission guard and zero-owner reservation remain held.
    pub fn git_worktree(
        &self,
        path: &str,
        op: GitWorktreeOp,
    ) -> Result<GitWorktreeResult, ApiError> {
        let common_dir = self.git.worktree_common_dir(Path::new(path))?;
        let patterns = if matches!(op, GitWorktreeOp::Add { .. }) {
            let source = self.git.worktree_checkout_root(Path::new(path))?;
            let source = source.to_str().ok_or_else(|| {
                ApiError::InvalidArgument("Invoking checkout path is not UTF-8".into())
            })?;
            self.settings_get_effective(Some(source.to_owned()))?
                .settings
                .git
                .worktree_copy
        } else {
            BTreeMap::new()
        };
        let mut result = self.git.worktree(Path::new(path), op, &patterns);
        if let Ok(GitWorktreeResult::Added { path, warnings, .. }) = &mut result {
            let remembered = Path::new(path)
                .parent()
                .ok_or_else(|| ApiError::Internal("Created worktree has no parent".into()))
                .and_then(|parent| {
                    serde_json::to_string(parent.to_str().ok_or_else(|| {
                        ApiError::InvalidArgument("Worktree parent is not UTF-8".into())
                    })?)
                    .map_err(|error| ApiError::Internal(error.to_string()))
                })
                .and_then(|value| {
                    self.app_state_put(format!("git:worktree-root:{common_dir}"), value)
                });
            if let Err(error) = remembered {
                warnings.push(format!(
                    "Worktree was created, but its root preference could not be saved: {error}"
                ));
            }
        }
        // Full refresh even after a partial migration failure. Loaded subfolders
        // are their own UI keys, so do not emit only checkout-root events.
        let mut refresh = BTreeSet::from([path.to_owned()]);
        for folder in self.git_watchers.watched_folders() {
            if self
                .git
                .worktree_common_dir(&folder)
                .is_ok_and(|common| common == common_dir)
            {
                if let Some(folder) = folder.to_str() {
                    refresh.insert(folder.to_owned());
                }
            }
        }
        for folder in refresh {
            self.git.invalidate(Path::new(&folder));
            self.emit(HostEvent::GitChanged(GitChangedEvent {
                folder_path: folder,
                scope: GitChangeScope::All,
                error: None,
            }));
        }
        result
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
    #[test]
    fn first_created_custom_worktree_root_is_shared_across_checkouts() {
        use super::*;
        use std::sync::Arc;
        struct Temporary(PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let temp = Temporary(std::env::temp_dir().join(format!(
            "sworm-worktree-preference-{}",
            uuid::Uuid::new_v4()
        )));
        let main = temp.0.join("main");
        std::fs::create_dir_all(&main).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(&main)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Sworm Test"]);
        git(&["config", "user.email", "test@sworm.invalid"]);
        git(&["config", "commit.gpgsign", "false"]);
        let hooks = temp.0.join("empty-hooks");
        std::fs::create_dir(&hooks).unwrap();
        git(&["config", "core.hooksPath", hooks.to_str().unwrap()]);
        std::fs::write(main.join("tracked.txt"), "committed\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-m", "initial"]);
        let host = Host::new(temp.0.join("state.db"), Arc::new(|_| Ok(()))).unwrap();
        let custom = temp.0.join("first custom root");
        let destination = custom.join("feature");
        let result = host
            .git_worktree(
                main.to_str().unwrap(),
                GitWorktreeOp::Add {
                    destination: destination.to_str().unwrap().to_owned(),
                    checkout: GitWorktreeCheckout::NewBranch {
                        name: "feature".into(),
                        base: "HEAD".into(),
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            result,
            GitWorktreeResult::Added {
                copy_complete: true,
                ..
            }
        ));
        let common = host
            .git_worktree_list(main.to_str().unwrap())
            .unwrap()
            .common_dir;
        let remembered: String = serde_json::from_str(
            &host
                .app_state_get(format!("git:worktree-root:{common}"))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(Path::new(&remembered), custom);
        for invoking in [&main, &destination] {
            let default = host
                .git_worktree_default_path(
                    invoking.to_str().unwrap(),
                    GitWorktreeCheckout::NewBranch {
                        name: "another".into(),
                        base: "HEAD".into(),
                    },
                    None,
                )
                .unwrap();
            assert_eq!(Path::new(&default), custom.join("another"));
        }
        // Explicit roots remain authoritative over the shared preference.
        let explicit = temp.0.join("override");
        assert_eq!(
            Path::new(
                &host
                    .git_worktree_default_path(
                        main.to_str().unwrap(),
                        GitWorktreeCheckout::Detached { rev: "HEAD".into() },
                        Some(explicit.to_str().unwrap().into())
                    )
                    .unwrap()
            )
            .parent(),
            Some(explicit.as_path())
        );
        drop(host);
    }
}
