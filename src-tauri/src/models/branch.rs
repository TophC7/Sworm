use serde::{Deserialize, Serialize};

/// A local branch (lives under `refs/heads/`) versus a remote-tracking
/// branch (lives under `refs/remotes/<remote>/`). Determined at parse
/// time from the ref namespace; the frontend uses it to gate context-
/// menu items (e.g. rename and set-upstream are local-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchKind {
    Local,
    Remote,
}

/// Current "in-flight" state of the working tree, derived directly
/// from `.git/` sentinel directories rather than parsing porcelain.
/// `Idle` covers normal day-to-day operation; the other variants drive
/// the paused-state status sub-row in the Branches view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchOpState {
    Idle,
    Rebasing,
    Merging,
}

/// Lightweight commit reference attached to each branch summary so the
/// list can show the tip's subject, author, and time without a second
/// round-trip per row. Hash is the full 40-char OID; `short_hash` is
/// the abbreviated form git already computed during `for-each-ref`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchCommitRef {
    pub hash: String,
    pub short_hash: String,
    pub subject: String,
    pub author: String,
    /// ISO-8601 author date, e.g. `2026-05-03T01:23:45-04:00`.
    pub date: String,
}

/// One row in the Branches view. Produced by `list_branches`,
/// `branch_info`, and used as the post-mutation refresh payload.
///
/// `upstream` is `None` for branches that have no tracking ref set;
/// in that case `ahead` and `behind` are zero rather than an error.
/// For remote-tracking branches the upstream is itself (e.g.
/// `origin/main`'s upstream is reported as `origin/main`) so callers
/// can render them uniformly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchSummary {
    pub name: String,
    pub kind: BranchKind,
    /// `true` when this branch is the currently checked-out HEAD.
    /// Always exactly one local branch (or none, on detached HEAD)
    /// has this set.
    pub is_current: bool,
    pub upstream: Option<String>,
    pub ahead: i32,
    pub behind: i32,
    pub tip: BranchCommitRef,
}
