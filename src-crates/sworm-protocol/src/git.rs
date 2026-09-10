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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Commit data for git graph rendering (includes parent hashes and refs).
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
pub struct CommitDetail {
    pub hash: String,
    pub short_hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    pub message: String,
    pub body: String,
    pub files: Vec<CommitFileChange>,
}

/// Single file entry within a commit.
#[derive(Debug, Clone, Serialize)]
pub struct CommitFileChange {
    pub path: String,
    pub status: String,
    pub additions: i32,
    pub deletions: i32,
}

/// A single stash entry with its file changes.
#[derive(Debug, Clone, Serialize)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
    pub date: String,
    pub files: Vec<CommitFileChange>,
}

pub const GIT_CHANGED_EVENT: &str = "git-changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GitChangeScope {
    Summary,
    All,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitChangedEvent {
    pub folder_path: String,
    pub scope: GitChangeScope,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitQuickDiffData {
    pub index_content: Option<String>,
    pub head_content: Option<String>,
    pub has_index_changes: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffFileContent {
    pub old_content: Option<String>,
    pub new_content: Option<String>,
    pub binary: bool,
}
