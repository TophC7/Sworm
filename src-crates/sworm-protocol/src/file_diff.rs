use serde::{Deserialize, Serialize};

/// File-level status for a diff entry. Converts from git's porcelain
/// single-letter codes (`M`, `A`, `D`, `R`, `C`, `U`, `?`) into an explicit
/// enum the frontend can match on without string-comparison fragility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Unmerged,
    Unknown,
}

impl GitStatus {
    /// Map git porcelain status codes (X/Y from `git status --porcelain=v2`)
    /// and numstat-derived signals into our enum.
    pub fn from_code(code: &str) -> Self {
        match code {
            "A" => Self::Added,
            "M" => Self::Modified,
            "D" => Self::Deleted,
            "R" => Self::Renamed,
            "C" => Self::Copied,
            "U" => Self::Unmerged,
            "?" => Self::Untracked,
            _ => Self::Unknown,
        }
    }
}

/// Where a diff is sourced from. One shape for every entry point ;
/// working tree, a commit, or a stash; so the frontend stays agnostic.
///
/// `staged` on `Working` disambiguates index vs worktree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DiffSource {
    Working {
        /// `true` → index vs HEAD. `false` → worktree vs index.
        staged: bool,
    },
    Commit {
        hash: String,
    },
    Stash {
        index: usize,
    },
}

/// Metadata for one changed file. Contents are loaded separately through
/// [`crate::git::DiffFileContent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    /// For renames/copies. `None` otherwise. Display as `old → new`.
    pub old_path: Option<String>,
    pub status: GitStatus,
    /// Monaco language id (`typescript`, `rust`, `python`, …). `plaintext`
    /// when unknown. Resolved server-side so the frontend doesn't need
    /// a parallel extension map.
    pub lang: String,
    /// Additions (numstat). `None` when git didn't report numstat (renames
    /// without edits, binary files).
    pub additions: Option<i32>,
    /// Deletions (numstat).
    pub deletions: Option<i32>,
}
