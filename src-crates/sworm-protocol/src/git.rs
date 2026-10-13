use serde::{Deserialize, Serialize};

/// Git change entry from `git status --porcelain=v2`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub additions: Option<i32>,
    pub deletions: Option<i32>,
}

/// Summary of git state for a project path.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitSummary {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub base_ref: Option<String>,
    pub ahead: Option<i32>,
    pub behind: Option<i32>,
    pub changes: Vec<GitChange>,
    pub staged_count: i32,
    pub unstaged_count: i32,
    pub untracked_count: i32,
    /// Upstream short name from `branch.<name>.merge` (e.g. `origin/main`); None without tracking config.
    pub upstream: Option<String>,
    /// HEAD is not on a branch (detached checkout, or mid-rebase).
    pub detached: bool,
    /// Paused merge/rebase/cherry-pick/revert, if any.
    pub operation: Option<GitOperation>,
    /// Unmerged paths with git's conflict kind.
    pub conflicts: Vec<GitConflict>,
    /// `MERGE_MSG`, else `SQUASH_MSG`, without `#` comment lines; None when absent or empty.
    pub message_template: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitOperationKind {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitOperation {
    pub kind: GitOperationKind,
    /// Merge: ref being merged. Rebase: ref rebased onto. Short hash when no ref points there; None for cherry-pick/revert.
    pub target: Option<String>,
    /// Rebase only: branch being rebased (from `head-name`); None for a detached rebase.
    pub head_name: Option<String>,
    /// Rebase only: 1-based current step and total.
    pub step: Option<u32>,
    pub total: Option<u32>,
    /// Commit being replayed (`REBASE_HEAD`, `CHERRY_PICK_HEAD`, `REVERT_HEAD`); None for merge.
    pub commit: Option<crate::branch::BranchCommitRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitConflictKind {
    BothModified,
    BothAdded,
    BothDeleted,
    AddedByUs,
    AddedByThem,
    DeletedByUs,
    DeletedByThem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConflict {
    pub path: String,
    pub kind: GitConflictKind,
}

/// Cheap git state for a Home project card: one `git status` pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitBrief {
    pub is_repo: bool,
    pub branch: Option<String>,
    /// Distinct changed paths, untracked included.
    pub changed: u32,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
}

/// Commit data for git graph rendering (includes parent hashes and refs).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphCommit {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub message: String,
    pub refs: Vec<String>,
}

/// Full commit detail for the commit-view page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitDetail {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub message: String,
    pub body: String,
    pub files: Vec<crate::file_diff::FileDiff>,
}

/// A single stash entry with its file changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub date: String,
    pub files: Vec<crate::file_diff::FileDiff>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitChangeScope {
    Summary,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitChangedEvent {
    pub folder_path: String,
    pub scope: GitChangeScope,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitQuickDiffData {
    pub index_content: Option<String>,
    pub head_content: Option<String>,
    pub has_index_changes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFileContent {
    pub old_content: Option<String>,
    pub new_content: Option<String>,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitPathsOp {
    /// `files: None` = whole repository.
    Stage {
        files: Option<Vec<String>>,
        allow_conflict_markers: bool,
    },
    Unstage {
        files: Option<Vec<String>>,
    },
    Discard {
        files: Option<Vec<String>>,
        permanent: bool,
    },
    /// Append repo-root-relative paths to the root `.gitignore`.
    Ignore {
        files: Vec<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitOptions {
    pub amend: bool,
    pub all: bool,
    pub sign_off: bool,
    pub no_verify: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitRemoteOp {
    Fetch {
        remote: Option<String>,
    },
    Pull {
        rebase: bool,
    },
    Push {
        branch: Option<String>,
        force: bool,
    },
    Publish {
        branch: Option<String>,
        remote: Option<String>,
    },
    Sync {
        rebase: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitStashScope {
    All { include_untracked: bool },
    Staged,
    Paths { paths: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitStashOp {
    Push {
        message: Option<String>,
        scope: GitStashScope,
    },
    Apply {
        index: usize,
    },
    Pop {
        index: usize,
    },
    Drop {
        index: usize,
    },
    Clear,
    Branch {
        index: usize,
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitBranchOp {
    Checkout {
        name: String,
    },
    CheckoutDetached {
        rev: String,
    },
    CheckoutRemote {
        remote_name: String,
        local_name: String,
    },
    Create {
        name: String,
        base: String,
        checkout: bool,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
    Delete {
        name: String,
        force: bool,
    },
    DeleteRemote {
        remote: String,
        name: String,
    },
    SetUpstream {
        branch: String,
        upstream: String,
    },
    UnsetUpstream {
        branch: String,
    },
    FastForward {
        name: String,
    },
    Merge {
        source: String,
        no_ff: bool,
    },
    Rebase {
        target: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitSequencerAction {
    Continue { message: Option<String> },
    Skip,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitConflictSide {
    Ours,
    Theirs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum GitConflictsOp {
    TakeSide {
        files: Vec<String>,
        side: GitConflictSide,
    },
    Delete {
        files: Vec<String>,
    },
}
