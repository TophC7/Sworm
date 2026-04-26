use crate::models::file_diff::{DiffSource, FileDiff, GitStatus};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tracing::warn;

/// Git change entry from `git status --porcelain=v2`.
#[derive(Debug, Clone, Serialize)]
pub struct GitChange {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub additions: Option<i32>,
    pub deletions: Option<i32>,
}

/// Summary of git state for a project path.
#[derive(Debug, Clone, Serialize)]
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

/// Hard limit on file content sent over IPC to prevent OOM on large files.
pub(crate) const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024; // 2 MiB

/// TTL for the [`GitService::get_summary`] cache. Coalesces bursts of
/// concurrent callers (status bar, sidebar, diff signature watchers,
/// `runGitAction` chasers) into a single git CLI sweep without
/// noticeably staling the UI.
const SUMMARY_CACHE_TTL: Duration = Duration::from_millis(300);

#[derive(Clone)]
struct CachedSummary {
    at: Instant,
    summary: GitSummary,
}

/// Git service using the system git CLI.
///
/// Holds an in-memory TTL cache of [`GitSummary`] values keyed by
/// project path. Mutating methods (`stage_*`, `commit`, `pull`, …) call
/// [`Self::invalidate`] so the next read picks up fresh state.
pub struct GitService {
    summary_cache: Mutex<HashMap<PathBuf, CachedSummary>>,
}

impl GitService {
    pub fn new() -> Self {
        Self {
            summary_cache: Mutex::new(HashMap::new()),
        }
    }

    fn cached_summary(&self, path: &Path) -> Option<GitSummary> {
        let cache = self.summary_cache.lock();
        let entry = cache.get(path)?;
        if entry.at.elapsed() < SUMMARY_CACHE_TTL {
            Some(entry.summary.clone())
        } else {
            None
        }
    }

    fn store_summary(&self, path: &Path, summary: &GitSummary) {
        self.summary_cache.lock().insert(
            path.to_path_buf(),
            CachedSummary {
                at: Instant::now(),
                summary: summary.clone(),
            },
        );
    }

    /// Drop any cached summary for `path`. Call after any mutation so the
    /// next [`Self::get_summary`] reflects the new state. Public so
    /// command-layer mutators that bypass [`Self::run_mutate`] (currently
    /// hunk staging in `commands/git.rs`) can still invalidate.
    pub fn invalidate(&self, path: &Path) {
        self.summary_cache.lock().remove(path);
    }

    /// Wrapper around [`run_git_mutate`] that invalidates the summary
    /// cache on success. Use this from any method that changes index or
    /// working-tree state.
    fn run_mutate(&self, path: &Path, args: &[&str]) -> Result<(), String> {
        let result = run_git_mutate(path, args);
        if result.is_ok() {
            self.invalidate(path);
        }
        result
    }

    /// Check if a path is inside a git work tree.
    pub fn is_git_repo(&self, path: &Path) -> bool {
        std::process::Command::new("git")
            .args(["--no-optional-locks", "rev-parse", "--is-inside-work-tree"])
            .current_dir(path)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Get the current branch name.
    pub fn current_branch(&self, path: &Path) -> Option<String> {
        let output = std::process::Command::new("git")
            .args(["--no-optional-locks", "branch", "--show-current"])
            .current_dir(path)
            .output()
            .ok()?;

        if output.status.success() {
            let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if branch.is_empty() {
                None
            } else {
                Some(branch)
            }
        } else {
            None
        }
    }

    /// Attempt to detect the default base ref (e.g. origin/main).
    pub fn default_base_ref(&self, path: &Path) -> Option<String> {
        // Try symbolic-ref first
        let output = std::process::Command::new("git")
            .args([
                "--no-optional-locks",
                "symbolic-ref",
                "--short",
                "refs/remotes/origin/HEAD",
            ])
            .current_dir(path)
            .output()
            .ok()?;

        if output.status.success() {
            let base = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !base.is_empty() {
                return Some(base);
            }
        }

        for candidate in &["origin/main", "origin/master"] {
            let check = std::process::Command::new("git")
                .args(["--no-optional-locks", "rev-parse", "--verify", candidate])
                .current_dir(path)
                .output()
                .ok()?;
            if check.status.success() {
                return Some(candidate.to_string());
            }
        }

        None
    }

    /// Get full git summary for a project path.
    ///
    /// Cached for [`SUMMARY_CACHE_TTL`] to coalesce concurrent callers
    /// (status bar, sidebar, diff signature watchers) into a single
    /// underlying CLI sweep. The four independent probes
    /// (`current_branch`, `default_base_ref`, `ahead_behind`,
    /// `get_changes`) run in parallel via `std::thread::scope` to cut
    /// wall-clock latency on dirty trees.
    pub fn get_summary(&self, path: &Path) -> GitSummary {
        if let Some(cached) = self.cached_summary(path) {
            return cached;
        }

        if !self.is_git_repo(path) {
            let summary = GitSummary {
                is_repo: false,
                branch: None,
                base_ref: None,
                ahead: None,
                behind: None,
                changes: vec![],
                staged_count: 0,
                unstaged_count: 0,
                untracked_count: 0,
            };
            self.store_summary(path, &summary);
            return summary;
        }

        let (branch, base_ref, ahead_behind, changes) = std::thread::scope(|s| {
            let b = s.spawn(|| self.current_branch(path));
            let r = s.spawn(|| self.default_base_ref(path));
            let a = s.spawn(|| self.ahead_behind(path));
            let c = s.spawn(|| self.get_changes(path));
            (
                b.join().unwrap_or(None),
                r.join().unwrap_or(None),
                a.join().unwrap_or((None, None)),
                c.join().unwrap_or_default(),
            )
        });
        let (ahead, behind) = ahead_behind;

        let staged_count = changes.iter().filter(|c| c.staged).count() as i32;
        let unstaged_count = changes
            .iter()
            .filter(|c| !c.staged && c.status != "?")
            .count() as i32;
        let untracked_count = changes.iter().filter(|c| c.status == "?").count() as i32;

        let summary = GitSummary {
            is_repo: true,
            branch,
            base_ref,
            ahead,
            behind,
            changes,
            staged_count,
            unstaged_count,
            untracked_count,
        };
        self.store_summary(path, &summary);
        summary
    }

    /// Get changed files using porcelain v2 + numstat.
    fn get_changes(&self, path: &Path) -> Vec<GitChange> {
        let mut changes = Vec::new();

        let output = std::process::Command::new("git")
            .args([
                "--no-optional-locks",
                "status",
                "--porcelain=v2",
                "-z",
                "--no-ahead-behind",
                "--untracked-files=all",
            ])
            .current_dir(path)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let mut records = output.stdout.split(|byte| *byte == 0);
                while let Some(record_bytes) = records.next() {
                    if record_bytes.is_empty() {
                        continue;
                    }

                    let record = String::from_utf8_lossy(record_bytes);
                    let kind = record.chars().next().unwrap_or(' ');

                    match kind {
                        '1' => {
                            let parts: Vec<&str> = record.split(' ').collect();
                            if parts.len() >= 9 {
                                let xy = parts[1];
                                push_status_entries(&mut changes, parts[8], xy);
                            }
                        }
                        '2' => {
                            let parts: Vec<&str> = record.split(' ').collect();
                            if parts.len() >= 10 {
                                push_status_entries(&mut changes, parts[9], parts[1]);
                                let _ = records.next();
                            }
                        }
                        'u' => {
                            let parts: Vec<&str> = record.split(' ').collect();
                            if parts.len() >= 11 {
                                push_status_entries(&mut changes, parts[10], parts[1]);
                            }
                        }
                        '?' => {
                            if record.len() > 2 {
                                changes.push(GitChange {
                                    path: record[2..].to_string(),
                                    status: "?".to_string(),
                                    staged: false,
                                    additions: None,
                                    deletions: None,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Numstat passes are conditional: skip the side that has no
        // entries from the porcelain pass; `git diff` with no changes
        // is still a process spawn we don't need to pay for.
        let has_unstaged = changes.iter().any(|c| !c.staged && c.status != "?");
        let has_staged = changes.iter().any(|c| c.staged);

        if has_unstaged {
            merge_numstat(&mut changes, path, &["diff", "--numstat"], false);
        }
        if has_staged {
            merge_numstat(&mut changes, path, &["diff", "--cached", "--numstat"], true);
        }

        changes
    }

    /// Get ahead/behind counts relative to the tracking branch.
    fn ahead_behind(&self, path: &Path) -> (Option<i32>, Option<i32>) {
        let output = std::process::Command::new("git")
            .args([
                "--no-optional-locks",
                "rev-list",
                "--left-right",
                "--count",
                "HEAD...@{upstream}",
            ])
            .current_dir(path)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                let parts: Vec<&str> = text.trim().split('\t').collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse::<i32>().ok();
                    let behind = parts[1].parse::<i32>().ok();
                    return (ahead, behind);
                }
            }
        }

        (None, None)
    }

    /// Get the combined patch for all working-tree changes (staged + unstaged).
    pub fn get_full_patch(&self, path: &Path) -> Option<String> {
        // Run staged and unstaged diffs in parallel; both are independent reads.
        let (staged, unstaged) = std::thread::scope(|s| {
            let sh = s.spawn(|| run_diff(path, &["diff", "--cached"]));
            let uh = s.spawn(|| run_diff(path, &["diff"]));
            (sh.join().ok().flatten(), uh.join().ok().flatten())
        });
        combine_patches(staged, unstaged)
    }

    /// Get patch for specific paths, scoped to one side.
    /// - `staged: Some(true)`: staged (index) diff only
    /// - `staged: Some(false)`: unstaged (working tree) diff only
    /// - `staged: None`: both combined
    pub fn get_path_patch(
        &self,
        path: &Path,
        files: &[String],
        staged: Option<bool>,
    ) -> Option<String> {
        if files.is_empty() {
            return None;
        }
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        let diff_args = |cached: bool| -> Vec<&str> {
            let mut args = vec!["diff"];
            if cached {
                args.push("--cached");
            }
            args.push("--");
            args.extend(refs.iter().copied());
            args
        };

        match staged {
            Some(true) => run_diff(path, &diff_args(true)),
            Some(false) => run_diff(path, &diff_args(false)),
            None => {
                let a = diff_args(true);
                let b = diff_args(false);
                let (s, u) = std::thread::scope(|sc| {
                    let sh = sc.spawn(|| run_diff(path, &a));
                    let uh = sc.spawn(|| run_diff(path, &b));
                    (sh.join().ok().flatten(), uh.join().ok().flatten())
                });
                combine_patches(s, u)
            }
        }
    }

    /// Get commit graph data for all branches (for graph visualization).
    pub fn get_graph(&self, path: &Path, limit: usize) -> Vec<GraphCommit> {
        let output = std::process::Command::new("git")
            .args([
                "--no-optional-locks",
                "log",
                "--all",
                "--topo-order",
                &format!("--max-count={}", limit),
                "--format=%H%n%h%n%P%n%an%n%aI%n%s%n%D",
            ])
            .current_dir(path)
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };

        if !output.status.success() {
            return Vec::new();
        }

        let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.to_string())
            .collect();

        lines
            .chunks(7)
            .filter_map(|chunk| {
                if chunk.len() != 7 {
                    return None;
                }

                let parents = if chunk[2].is_empty() {
                    Vec::new()
                } else {
                    chunk[2].split(' ').map(|s| s.to_string()).collect()
                };

                let refs = if chunk[6].is_empty() {
                    Vec::new()
                } else {
                    chunk[6].split(", ").map(|s| s.trim().to_string()).collect()
                };

                Some(GraphCommit {
                    hash: chunk[0].clone(),
                    short_hash: chunk[1].clone(),
                    parents,
                    author: chunk[3].clone(),
                    date: chunk[4].clone(),
                    message: chunk[5].clone(),
                    refs,
                })
            })
            .collect()
    }

    /// Get full commit detail (info + changed files with stats).
    pub fn get_commit_detail(&self, path: &Path, hash: &str) -> Option<CommitDetail> {
        // 1. Commit metadata
        // Use null-byte delimiters so %b (body) can contain newlines safely.
        let info = std::process::Command::new("git")
            .args([
                "show",
                "-s",
                "--format=%H%x00%h%x00%P%x00%an%x00%aI%x00%s%x00%b",
                hash,
            ])
            .current_dir(path)
            .output()
            .ok()?;

        if !info.status.success() {
            return None;
        }

        let info_text = String::from_utf8_lossy(&info.stdout);
        let parts: Vec<&str> = info_text.splitn(7, '\0').collect();
        if parts.len() < 6 {
            return None;
        }

        let parents: Vec<String> = if parts[2].is_empty() {
            Vec::new()
        } else {
            parts[2].split(' ').map(|s| s.to_string()).collect()
        };

        // 2. File stats; diff against first parent (like GitHub).
        //    For root commits, diff-tree --root shows everything as added.
        let diff_ref = if parents.is_empty() {
            // Root commit; use diff-tree --root
            String::new()
        } else {
            format!("{}^..{}", hash, hash)
        };

        let (numstat_lines, status_lines) = if diff_ref.is_empty() {
            let ns = std::process::Command::new("git")
                .args([
                    "diff-tree",
                    "--root",
                    "-r",
                    "--no-commit-id",
                    "--numstat",
                    hash,
                ])
                .current_dir(path)
                .output()
                .ok()?;
            let st = std::process::Command::new("git")
                .args([
                    "diff-tree",
                    "--root",
                    "-r",
                    "--no-commit-id",
                    "--name-status",
                    hash,
                ])
                .current_dir(path)
                .output()
                .ok()?;
            (
                String::from_utf8_lossy(&ns.stdout).to_string(),
                String::from_utf8_lossy(&st.stdout).to_string(),
            )
        } else {
            let ns = std::process::Command::new("git")
                .args(["diff", "--numstat", &diff_ref])
                .current_dir(path)
                .output()
                .ok()?;
            let st = std::process::Command::new("git")
                .args(["diff", "--name-status", &diff_ref])
                .current_dir(path)
                .output()
                .ok()?;
            (
                String::from_utf8_lossy(&ns.stdout).to_string(),
                String::from_utf8_lossy(&st.stdout).to_string(),
            )
        };

        // Parse numstat: "10\t5\tpath/to/file"
        let mut stats = std::collections::HashMap::<String, (i32, i32)>::new();
        for line in numstat_lines.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 3 {
                let adds = parts[0].parse().unwrap_or(0);
                let dels = parts[1].parse().unwrap_or(0);
                stats.insert(parts[2].to_string(), (adds, dels));
            }
        }

        // Parse name-status: "M\tpath" or "R100\told\tnew"
        let mut files: Vec<CommitFileChange> = Vec::new();
        for line in status_lines.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let status = parts[0].chars().next().unwrap_or('M').to_string();
                let file_path = parts[parts.len() - 1].to_string();
                let (additions, deletions) = stats.get(&file_path).copied().unwrap_or((0, 0));
                files.push(CommitFileChange {
                    path: file_path,
                    status,
                    additions,
                    deletions,
                });
            }
        }

        Some(CommitDetail {
            hash: parts[0].to_string(),
            short_hash: parts[1].to_string(),
            parents,
            author: parts[3].to_string(),
            date: parts[4].to_string(),
            message: parts[5].to_string(),
            body: parts.get(6).unwrap_or(&"").trim().to_string(),
            files,
        })
    }

    // ── Write operations ────────────────────────────────────────────

    /// Stage all changes (tracked + untracked).
    pub fn stage_all(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["add", "-A"])
    }

    /// Stage specific files or directories.
    pub fn stage_files(&self, path: &Path, files: &[String]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["add", "--"];
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(refs);
        self.run_mutate(path, &args)
    }

    /// Unstage all staged changes back to the working tree.
    pub fn unstage_all(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["reset", "HEAD"])
    }

    /// Unstage specific files or directories.
    pub fn unstage_files(&self, path: &Path, files: &[String]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["reset", "HEAD", "--"];
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(refs);
        self.run_mutate(path, &args)
    }

    /// Discard all unstaged changes and remove untracked files.
    pub fn discard_all(&self, path: &Path) -> Result<(), String> {
        // Restore tracked files to HEAD state
        self.run_mutate(path, &["checkout", "--", "."])?;
        // Remove untracked files and directories (but not ignored ones)
        self.run_mutate(path, &["clean", "-fd"])
    }

    /// Discard changes for specific files or directories.
    /// Handles both tracked (checkout) and untracked (clean) files in one
    /// invocation each. Errors are ignored because a mixed set of paths
    /// will always fail one of the two commands; e.g. checkout rejects
    /// untracked paths, clean is a no-op for tracked ones.
    pub fn discard_files(&self, path: &Path, files: &[String]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

        let mut checkout_args = vec!["checkout", "--"];
        checkout_args.extend(refs.iter().copied());
        let _ = self.run_mutate(path, &checkout_args);

        let mut clean_args = vec!["clean", "-fd", "--"];
        clean_args.extend(refs.iter().copied());
        let _ = self.run_mutate(path, &clean_args);

        Ok(())
    }

    /// Create a commit with the given message. Returns the new short hash.
    pub fn commit(&self, path: &Path, message: &str) -> Result<String, String> {
        self.run_mutate(path, &["commit", "-m", message])?;
        // Read back the new commit hash
        let output = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .current_dir(path)
            .output()
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Soft-reset the last commit, preserving changes as staged, and
    /// return the message that was on that commit so the UI can restore
    /// it into the commit textarea for easy editing or re-use.
    pub fn undo_last_commit(&self, path: &Path) -> Result<String, String> {
        // Snapshot the message BEFORE resetting. `%B` is the full raw
        // body (subject + body), which matches what the user originally
        // typed into the textarea. `trim_end` removes both `\n` and
        // `\r\n` so the textarea doesn't inherit a stray CR on
        // CRLF-flavoured repos.
        let output = std::process::Command::new("git")
            .args(["log", "-1", "--format=%B", "HEAD"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("Failed to read last commit message: {}", e))?;
        let message = if output.status.success() {
            String::from_utf8_lossy(&output.stdout)
                .trim_end()
                .to_string()
        } else {
            warn!(
                "git log for undo_last_commit exited non-zero ({}), proceeding with empty restore",
                output.status
            );
            String::new()
        };

        self.run_mutate(path, &["reset", "--soft", "HEAD~1"])?;
        Ok(message)
    }

    /// Push current branch to its upstream remote.
    pub fn push(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["push"])
    }

    /// Push with --force-with-lease (safe force push).
    pub fn push_force_with_lease(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["push", "--force-with-lease"])
    }

    /// Pull from the upstream remote (fetch + merge).
    pub fn pull(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["pull"])
    }

    /// Fetch from all remotes.
    pub fn fetch(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["fetch", "--all"])
    }

    /// Stash all changes including untracked files.
    pub fn stash_all(&self, path: &Path, message: Option<&str>) -> Result<(), String> {
        let mut args = vec!["stash", "push", "--include-untracked"];
        if let Some(msg) = message {
            args.push("-m");
            args.push(msg);
        }
        self.run_mutate(path, &args)
    }

    /// Count stash entries without fetching per-entry file stats.
    pub fn stash_count(&self, path: &Path) -> Result<usize, String> {
        let output = std::process::Command::new("git")
            .args(["--no-optional-locks", "stash", "list"])
            .current_dir(path)
            .output()
            .map_err(|e| e.to_string())?;

        if !output.status.success() {
            return Ok(0);
        }

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().filter(|l| !l.is_empty()).count())
    }

    /// List all stash entries with their changed files.
    pub fn stash_list(&self, path: &Path) -> Vec<StashEntry> {
        let output = std::process::Command::new("git")
            .args(["stash", "list", "--format=%gd%x00%gs%x00%aI"])
            .current_dir(path)
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };
        if !output.status.success() {
            return Vec::new();
        }

        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(3, '\0').collect();
                if parts.len() < 3 {
                    return None;
                }
                // parts[0] = "stash@{N}", extract N
                let idx_str = parts[0]
                    .strip_prefix("stash@{")
                    .and_then(|s| s.strip_suffix('}'))?;
                let index: usize = idx_str.parse().ok()?;

                let files = self.stash_files(path, index);

                Some(StashEntry {
                    index,
                    message: parts[1].to_string(),
                    date: parts[2].to_string(),
                    files,
                })
            })
            .collect()
    }

    /// Get changed files for a specific stash entry.
    fn stash_files(&self, path: &Path, index: usize) -> Vec<CommitFileChange> {
        let stash_ref = format!("stash@{{{}}}", index);

        // numstat for line counts
        let ns_output = std::process::Command::new("git")
            .args(["stash", "show", "--numstat", &stash_ref])
            .current_dir(path)
            .output();

        // name-status for change type
        let st_output = std::process::Command::new("git")
            .args(["stash", "show", "--name-status", &stash_ref])
            .current_dir(path)
            .output();

        let mut stats = std::collections::HashMap::<String, (i32, i32)>::new();
        if let Ok(ref o) = ns_output {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 3 {
                        let adds = parts[0].parse().unwrap_or(0);
                        let dels = parts[1].parse().unwrap_or(0);
                        stats.insert(parts[2].to_string(), (adds, dels));
                    }
                }
            }
        }

        let mut files = Vec::new();
        if let Ok(ref o) = st_output {
            if o.status.success() {
                for line in String::from_utf8_lossy(&o.stdout).lines() {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 2 {
                        let status = parts[0].chars().next().unwrap_or('M').to_string();
                        let file_path = parts[parts.len() - 1].to_string();
                        let (additions, deletions) =
                            stats.get(&file_path).copied().unwrap_or((0, 0));
                        files.push(CommitFileChange {
                            path: file_path,
                            status,
                            additions,
                            deletions,
                        });
                    }
                }
            }
        }

        files
    }

    /// Pop (apply + drop) a stash entry by index.
    pub fn stash_pop(&self, path: &Path, index: usize) -> Result<(), String> {
        let stash_ref = format!("stash@{{{}}}", index);
        self.run_mutate(path, &["stash", "pop", &stash_ref])
    }

    /// Drop a stash entry by index without applying it.
    pub fn stash_drop(&self, path: &Path, index: usize) -> Result<(), String> {
        let stash_ref = format!("stash@{{{}}}", index);
        self.run_mutate(path, &["stash", "drop", &stash_ref])
    }

    /// Get all file diffs for a stash entry (including untracked files).
    /// Initialize a new git repository. Repo identity changes (now a
    /// repo), so [`Self::run_mutate`]'s invalidation drops any stale
    /// "not a repo" summary on next read.
    pub fn init(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["init"])
    }

    /// Clone a repository into the given directory (in-place, no subfolder).
    ///
    /// Uses `git clone <url> .` for empty dirs, or init + remote + fetch +
    /// checkout for dirs with existing content. Each step routes through
    /// [`Self::run_mutate`] so the summary cache is invalidated even on
    /// partial completion.
    pub fn clone_in_place(&self, path: &Path, url: &str) -> Result<(), String> {
        let has_content = path
            .read_dir()
            .map_err(|e| format!("Cannot read directory: {}", e))?
            .any(|entry| {
                entry
                    .map(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .unwrap_or(false)
            });

        if !has_content {
            return self.run_mutate(path, &["clone", url, "."]);
        }

        self.run_mutate(path, &["init"])?;
        self.run_mutate(path, &["remote", "add", "origin", url])?;
        self.run_mutate(path, &["fetch", "origin"])?;
        let default_branch = self.detect_remote_default_branch(path);
        self.run_mutate(
            path,
            &[
                "checkout",
                "-b",
                &default_branch,
                &format!("origin/{}", default_branch),
            ],
        )
    }

    /// Read the default branch from the local ref set by fetch, avoiding a network call.
    fn detect_remote_default_branch(&self, path: &Path) -> String {
        // After `git fetch origin`, the remote HEAD symref is available locally.
        let output = std::process::Command::new("git")
            .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
            .current_dir(path)
            .output();

        if let Ok(output) = output {
            if output.status.success() {
                let refstr = String::from_utf8_lossy(&output.stdout);
                if let Some(branch) = refstr.trim().strip_prefix("refs/remotes/origin/") {
                    return branch.to_string();
                }
            }
        }

        "main".to_string()
    }

    // ── Unified diff payload for the Monaco multi-file diff viewer ──
    //
    // `get_diff_files` is the single entry point used by the new viewer.
    // It returns `{old, new}` content per file so the frontend can build
    // two `ITextModel`s and hand them to a `DiffEditor`. All three diff
    // sources (working tree, a commit, a stash) funnel through here.

    /// Return every changed file for the given source, with both sides
    /// of content attached. Skips files that are binary or oversized
    /// (still present in the list, but with `content: None` + `binary: true`).
    pub fn get_diff_files(&self, path: &Path, source: &DiffSource) -> Vec<FileDiff> {
        if !self.is_git_repo(path) {
            return Vec::new();
        }
        match source {
            DiffSource::Working { staged } => self.working_diff_files(path, *staged),
            DiffSource::Commit { hash } => self.commit_diff_files(path, hash),
            DiffSource::Stash { index } => self.stash_diff_files(path, *index),
        }
    }

    /// Cheap index for the working tree: file list + metadata, no
    /// content. Pair with [`Self::get_working_diff_file_content`] to
    /// load each file lazily, avoiding a multi-megabyte payload to the
    /// frontend before the user has expanded any row.
    pub fn get_working_diff_index(&self, path: &Path, staged: bool) -> Vec<FileDiff> {
        if !self.is_git_repo(path) {
            return Vec::new();
        }
        filter_working_changes(self.get_changes(path), Some(staged))
            .into_iter()
            .map(|change| FileDiff {
                path: change.path.clone(),
                old_path: None,
                status: GitStatus::from_code(&change.status),
                lang: lang_from_path(&change.path).to_string(),
                old_content: None,
                new_content: None,
                binary: false,
                additions: change.additions,
                deletions: change.deletions,
            })
            .collect()
    }

    /// Per-file content for a working-tree diff. Caller passes the
    /// file's status (so deletions, additions, and untracked variants
    /// pick the right read strategy) and the staged side.
    pub fn get_working_diff_file_content(
        &self,
        path: &Path,
        file_path: &str,
        status: GitStatus,
        staged: bool,
    ) -> (Option<String>, Option<String>, bool) {
        self.working_file_contents(path, file_path, status, staged)
    }

    fn working_diff_files(&self, path: &Path, staged_filter: Option<bool>) -> Vec<FileDiff> {
        filter_working_changes(self.get_changes(path), staged_filter)
            .into_iter()
            .map(|change| {
                let status = GitStatus::from_code(&change.status);
                let (old_content, new_content, binary) =
                    self.working_file_contents(path, &change.path, status, change.staged);
                FileDiff {
                    path: change.path.clone(),
                    old_path: None,
                    status,
                    lang: lang_from_path(&change.path).to_string(),
                    old_content,
                    new_content,
                    binary,
                    additions: change.additions,
                    deletions: change.deletions,
                }
            })
            .collect()
    }

    fn working_file_contents(
        &self,
        path: &Path,
        file_path: &str,
        status: GitStatus,
        staged: bool,
    ) -> (Option<String>, Option<String>, bool) {
        match status {
            GitStatus::Untracked => {
                // Untracked file: no old side.
                let blob = read_worktree_blob(path, file_path);
                match blob {
                    BlobResult::Text(s) => (None, Some(s), false),
                    BlobResult::Binary | BlobResult::Oversized => (None, None, true),
                    BlobResult::Missing => (None, None, false),
                }
            }
            GitStatus::Deleted => {
                // Deletion: only an old side.
                let old = if staged {
                    read_git_show_blob(path, &format!("HEAD:{}", file_path))
                } else {
                    read_git_show_blob(path, &format!(":{}", file_path))
                };
                fold_single_side(old, true)
            }
            GitStatus::Added if staged => {
                // Staged addition: index has it, HEAD does not.
                let new = read_git_show_blob(path, &format!(":{}", file_path));
                fold_single_side(new, false)
            }
            _ => {
                if staged {
                    let old = read_git_show_blob(path, &format!("HEAD:{}", file_path));
                    let new = read_git_show_blob(path, &format!(":{}", file_path));
                    fold_both_sides(old, new)
                } else {
                    // Unstaged: index vs worktree. Fall back to HEAD when
                    // the index doesn't have an entry (rare edge with
                    // intent-to-add files).
                    let old = match read_git_show_blob(path, &format!(":{}", file_path)) {
                        BlobResult::Missing => {
                            read_git_show_blob(path, &format!("HEAD:{}", file_path))
                        }
                        other => other,
                    };
                    let new = read_worktree_blob(path, file_path);
                    fold_both_sides(old, new)
                }
            }
        }
    }

    fn commit_diff_files(&self, path: &Path, hash: &str) -> Vec<FileDiff> {
        // Parents decide whether this is a root commit (one-sided diff)
        // or a normal commit diffed against its first parent.
        let parents_out = std::process::Command::new("git")
            .args(["rev-list", "--parents", "-n", "1", hash])
            .current_dir(path)
            .output();
        let is_root = match parents_out {
            Ok(o) if o.status.success() => {
                let line = String::from_utf8_lossy(&o.stdout).trim().to_string();
                // Output is "<hash> <parent1> <parent2> …". No parents → root.
                line.split_whitespace().count() <= 1
            }
            _ => false,
        };
        let old_ref = if is_root {
            None
        } else {
            Some(format!("{}^", hash))
        };

        let files = self.list_rev_files(path, hash, old_ref.as_deref());
        let mut out = Vec::new();
        for entry in files {
            let old_content = match (&old_ref, &entry.old_path_for_diff) {
                (Some(old_rev), Some(op)) => {
                    read_git_show_blob(path, &format!("{}:{}", old_rev, op))
                }
                _ => BlobResult::Missing,
            };
            let new_content = if matches!(entry.status, GitStatus::Deleted) {
                BlobResult::Missing
            } else {
                read_git_show_blob(path, &format!("{}:{}", hash, entry.path))
            };
            let (old, new, binary) = fold_both_sides(old_content, new_content);
            // Language prefers the new path; fall back to the old path for
            // deletions where `path` itself may be empty.
            let lang = if !entry.path.is_empty() {
                lang_from_path(&entry.path)
            } else {
                lang_from_path(entry.old_path_for_diff.as_deref().unwrap_or(""))
            };
            out.push(FileDiff {
                path: entry.path,
                old_path: entry.old_path,
                status: entry.status,
                lang: lang.to_string(),
                old_content: old,
                new_content: new,
                binary,
                additions: entry.additions,
                deletions: entry.deletions,
            });
        }
        out
    }

    fn stash_diff_files(&self, path: &Path, index: usize) -> Vec<FileDiff> {
        let stash_ref = format!("stash@{{{}}}", index);
        let old_ref = format!("{}^", stash_ref);

        // `git stash show --name-status --numstat` lists both tracked and
        // (with -u) untracked changes in the stash. Untracked-only entries
        // live on the third parent `stash@{N}^3`; we treat those as adds.
        let status_out = std::process::Command::new("git")
            .args(["stash", "show", "-u", "-z", "--name-status", &stash_ref])
            .current_dir(path)
            .output();
        let numstat_out = std::process::Command::new("git")
            .args(["stash", "show", "-u", "-z", "--numstat", &stash_ref])
            .current_dir(path)
            .output();

        let status_text = match status_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        };
        let numstat_text = match numstat_out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        };

        let stats = parse_numstat(&numstat_text);
        let entries = parse_name_status(&status_text);

        let mut out = Vec::new();
        for entry in entries {
            let (additions, deletions) = stats
                .get(&entry.path)
                .copied()
                .map(|(a, d)| (Some(a), Some(d)))
                .unwrap_or((None, None));

            // Try reading from the stash's first parent for the old side.
            // If that fails (untracked files have no pre-image), treat as add.
            let old_content = match entry.status {
                GitStatus::Added | GitStatus::Untracked => BlobResult::Missing,
                _ => {
                    let source_path = entry.old_path_for_diff.as_deref().unwrap_or(&entry.path);
                    read_git_show_blob(path, &format!("{}:{}", old_ref, source_path))
                }
            };
            let new_content = if matches!(entry.status, GitStatus::Deleted) {
                BlobResult::Missing
            } else {
                read_git_show_blob(path, &format!("{}:{}", stash_ref, entry.path))
            };
            let (old, new, binary) = fold_both_sides(old_content, new_content);

            out.push(FileDiff {
                path: entry.path.clone(),
                old_path: entry.old_path,
                status: entry.status,
                lang: lang_from_path(&entry.path).to_string(),
                old_content: old,
                new_content: new,
                binary,
                additions,
                deletions,
            });
        }
        out
    }

    /// Parse name-status + numstat for a rev range so we get per-file
    /// status + rename detection + line counts in one place. Used by
    /// commit diffs (and reusable for other rev-based callers later).
    fn list_rev_files(
        &self,
        path: &Path,
        new_rev: &str,
        old_rev: Option<&str>,
    ) -> Vec<RevFileEntry> {
        // `-z` keeps paths NUL-delimited. Without it, paths with tabs /
        // newlines misparse, AND renames collapse into a single
        // `{old => new}` field that breaks numstat lookups by new path.
        let (status_text, numstat_text) = match old_rev {
            Some(old) => {
                let range = format!("{}..{}", old, new_rev);
                (
                    run_git_capture(path, &["diff", "--name-status", "-z", "-M", "-C", &range]),
                    run_git_capture(path, &["diff", "--numstat", "-z", &range]),
                )
            }
            None => (
                run_git_capture(
                    path,
                    &[
                        "diff-tree",
                        "--root",
                        "-r",
                        "-z",
                        "--no-commit-id",
                        "--name-status",
                        new_rev,
                    ],
                ),
                run_git_capture(
                    path,
                    &[
                        "diff-tree",
                        "--root",
                        "-r",
                        "-z",
                        "--no-commit-id",
                        "--numstat",
                        new_rev,
                    ],
                ),
            ),
        };

        let stats = parse_numstat(&numstat_text);
        let mut entries = parse_name_status(&status_text);
        for entry in entries.iter_mut() {
            if let Some((a, d)) = stats.get(&entry.path).copied() {
                entry.additions = Some(a);
                entry.deletions = Some(d);
            }
        }
        entries
    }
}

/// Intermediate rev-walk entry. Keeps the old-path separate so the
/// caller can resolve content from the old rev with the correct name.
struct RevFileEntry {
    path: String,
    /// User-facing old path for renames/copies (`Some("old")` when
    /// different from `path`, `None` otherwise).
    old_path: Option<String>,
    /// The path to feed into `git show <old_rev>:<path>`. Same as
    /// `old_path.or(path)` but stored explicitly so rename logic stays
    /// in parsing, not reading.
    old_path_for_diff: Option<String>,
    status: GitStatus,
    additions: Option<i32>,
    deletions: Option<i32>,
}

/// Outcome of reading a blob: text, binary, oversized, or missing.
/// The command layer folds the non-text variants into `content: None`
/// + `binary: true` (oversized) / `binary: false` (missing).
enum BlobResult {
    Text(String),
    Binary,
    Oversized,
    Missing,
}

/// Fold paired blob results into `(old, new, binary)` for `FileDiff`.
/// Binary OR oversized on either side → `binary = true`; the frontend
/// shows a placeholder instead of mounting Monaco.
fn fold_both_sides(old: BlobResult, new: BlobResult) -> (Option<String>, Option<String>, bool) {
    let binary = matches!(old, BlobResult::Binary | BlobResult::Oversized)
        || matches!(new, BlobResult::Binary | BlobResult::Oversized);
    let to_opt = |b: BlobResult| -> Option<String> {
        match b {
            BlobResult::Text(s) => Some(s),
            _ => None,
        }
    };
    (to_opt(old), to_opt(new), binary)
}

/// Single-sided fold: `on_old` selects whether the blob is the old or new side.
fn fold_single_side(blob: BlobResult, on_old: bool) -> (Option<String>, Option<String>, bool) {
    let binary = matches!(blob, BlobResult::Binary | BlobResult::Oversized);
    let text = match blob {
        BlobResult::Text(s) => Some(s),
        _ => None,
    };
    if on_old {
        (text, None, binary)
    } else {
        (None, text, binary)
    }
}

/// Read a blob via `git show` (supports `HEAD:path`, `:path`, `<ref>:path`).
fn read_git_show_blob(path: &Path, spec: &str) -> BlobResult {
    let output = std::process::Command::new("git")
        .args(["--no-optional-locks", "show", spec])
        .current_dir(path)
        .output();
    let output = match output {
        Ok(o) => o,
        Err(_) => return BlobResult::Missing,
    };
    if !output.status.success() {
        return BlobResult::Missing;
    }
    classify_bytes(output.stdout)
}

/// Read a blob directly from the working tree.
fn read_worktree_blob(project: &Path, rel: &str) -> BlobResult {
    let full = project.join(rel);
    let meta = match std::fs::metadata(&full) {
        Ok(m) => m,
        Err(_) => return BlobResult::Missing,
    };
    if meta.len() > MAX_CONTENT_BYTES as u64 {
        return BlobResult::Oversized;
    }
    match std::fs::read(&full) {
        Ok(bytes) => classify_bytes(bytes),
        Err(_) => BlobResult::Missing,
    }
}

/// Classify a raw byte buffer as text / binary / oversized.
/// Mirrors `guard_content` but preserves the binary vs oversize distinction
/// so the frontend can flag binaries explicitly.
fn classify_bytes(bytes: Vec<u8>) -> BlobResult {
    if bytes.len() > MAX_CONTENT_BYTES {
        return BlobResult::Oversized;
    }
    let probe = bytes.len().min(8192);
    if bytes[..probe].contains(&0) {
        return BlobResult::Binary;
    }
    match String::from_utf8(bytes) {
        Ok(s) => BlobResult::Text(s),
        Err(_) => BlobResult::Binary,
    }
}

/// Parse `git diff --numstat -z` output into `{path: (additions, deletions)}`.
///
/// `-z` format records are NUL-terminated:
///   regular: `adds\tdels\tpath\0`
///   rename:  `adds\tdels\t\0oldpath\0newpath\0`   (empty path field, then two paths)
///
/// Keyed by the POST-rename path so callers can look up by `entry.path`.
/// Binary entries (`-\t-\tpath`) skip and surface as `None` upstream.
fn parse_numstat(text: &str) -> std::collections::HashMap<String, (i32, i32)> {
    let mut map = std::collections::HashMap::new();
    let mut fields = text.split('\0').filter(|s| !s.is_empty()).peekable();

    while let Some(head) = fields.next() {
        // `head` is either "adds\tdels\tpath" or "adds\tdels\t" (rename prefix).
        let tab_parts: Vec<&str> = head.splitn(3, '\t').collect();
        if tab_parts.len() < 3 {
            continue;
        }
        let Ok(adds) = tab_parts[0].parse::<i32>() else {
            continue;
        };
        let Ok(dels) = tab_parts[1].parse::<i32>() else {
            continue;
        };
        let path = if tab_parts[2].is_empty() {
            // Rename: next two fields are old, new.
            let _old = fields.next();
            let Some(new_path) = fields.next() else {
                continue;
            };
            new_path.to_string()
        } else {
            tab_parts[2].to_string()
        };
        map.insert(path, (adds, dels));
    }
    map
}

/// Parse `git diff --name-status -z -M -C` output into structured entries.
///
/// `-z` format alternates status and path(s), all NUL-delimited:
///   regular: `<status>\0<path>\0`
///   rename:  `R<score>\0<oldpath>\0<newpath>\0`
fn parse_name_status(text: &str) -> Vec<RevFileEntry> {
    let mut out = Vec::new();
    let mut fields = text.split('\0').filter(|s| !s.is_empty());

    while let Some(code_raw) = fields.next() {
        let code_letter = code_raw.get(0..1).unwrap_or("M");
        let status = GitStatus::from_code(code_letter);

        let (new_path, old_path) = match status {
            GitStatus::Renamed | GitStatus::Copied => {
                let Some(old) = fields.next() else { break };
                let Some(new) = fields.next() else { break };
                (new.to_string(), Some(old.to_string()))
            }
            _ => {
                let Some(p) = fields.next() else { break };
                (p.to_string(), None)
            }
        };
        let old_path_for_diff = old_path.clone().or_else(|| Some(new_path.clone()));

        out.push(RevFileEntry {
            path: new_path,
            old_path,
            old_path_for_diff,
            status,
            additions: None,
            deletions: None,
        });
    }
    out
}

/// Filter and dedupe `git status --porcelain` entries for a working-tree
/// diff. `staged_filter`:
///   * `None`: include both sides; dedupe by `(path, staged)` so a tracked
///     file appearing as both staged and unstaged keeps both copies.
///   * `Some(want)`: restrict to that side only; dedupe by `path` since
///     only one copy survives.
/// Untracked files always live on the unstaged side and are dropped from
/// the staged-side filter.
fn filter_working_changes(changes: Vec<GitChange>, staged_filter: Option<bool>) -> Vec<GitChange> {
    let mut seen_str = std::collections::HashSet::<String>::new();
    let mut seen_pair = std::collections::HashSet::<(String, bool)>::new();
    let mut out = Vec::new();
    for change in changes {
        if change.status == "?" && matches!(staged_filter, Some(true)) {
            continue;
        }
        if let Some(want) = staged_filter {
            if change.status != "?" && change.staged != want {
                continue;
            }
            if !seen_str.insert(change.path.clone()) {
                continue;
            }
        } else if !seen_pair.insert((change.path.clone(), change.staged)) {
            continue;
        }
        out.push(change);
    }
    out
}

/// Run a git command and return stdout as a String (empty on failure).
fn run_git_capture(path: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(std::iter::once("--no-optional-locks").chain(args.iter().copied()))
        .current_dir(path)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => String::new(),
    }
}

/// Resolve a Monaco language id from a file path. Falls back to
/// `plaintext` for unknown extensions and dotfiles we don't special-case.
/// Kept intentionally narrow; add only languages that have real support
/// in the loaded Monaco bundle (see `src/lib/editor/monacoEnv.ts`).
fn lang_from_path(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let basename = std::path::Path::new(&lower)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower);

    // Special-case common extensionless files before extension match.
    match basename {
        "dockerfile" => return "dockerfile",
        "makefile" | "gnumakefile" => return "makefile",
        "cmakelists.txt" => return "cmake",
        ".gitignore" | ".gitattributes" | ".editorconfig" => return "plaintext",
        _ => {}
    }

    let ext = std::path::Path::new(&lower)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    match ext {
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescript",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascript",
        "json" | "jsonc" => "json",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "scss",
        "less" => "less",
        "svelte" => "svelte",
        "vue" => "html",
        "rs" => "rust",
        "go" => "go",
        "py" | "pyi" => "python",
        "rb" => "ruby",
        "java" => "java",
        "kt" | "kts" => "kotlin",
        "swift" => "swift",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" => "cpp",
        "m" | "mm" => "objective-c",
        "cs" => "csharp",
        "php" => "php",
        "sh" | "bash" | "zsh" => "shell",
        "fish" => "fish",
        "ps1" => "powershell",
        "lua" => "lua",
        "sql" => "sql",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "ini" | "cfg" => "ini",
        "xml" | "svg" => "xml",
        "md" | "markdown" => "markdown",
        "nix" => "nix",
        "dart" => "dart",
        "r" => "r",
        "scala" | "sc" => "scala",
        "ex" | "exs" => "elixir",
        "erl" | "hrl" => "erlang",
        "hs" => "haskell",
        "clj" | "cljs" | "cljc" | "edn" => "clojure",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        "tex" => "latex",
        _ => "plaintext",
    }
}

/// Run a `git diff` variant and return stdout as a String, or `None` if empty.
fn run_diff(path: &Path, args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .ok()?;
    let body = String::from_utf8_lossy(&output.stdout).into_owned();
    if body.trim().is_empty() {
        None
    } else {
        Some(body)
    }
}

/// Concatenate two optional patch bodies with a blank line separator.
fn combine_patches(staged: Option<String>, unstaged: Option<String>) -> Option<String> {
    match (staged, unstaged) {
        (None, None) => None,
        (Some(s), None) => Some(s),
        (None, Some(u)) => Some(u),
        (Some(s), Some(u)) => Some(format!("{}\n{}", s, u)),
    }
}

/// Run a mutating git command, returning `Ok(())` on success or
/// `Err(stderr)` on failure.
fn run_git_mutate(path: &Path, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn push_status_entries(changes: &mut Vec<GitChange>, file_path: &str, xy: &str) {
    let mut statuses = xy.chars();
    let index = statuses.next().unwrap_or(' ');
    let worktree = statuses.next().unwrap_or(' ');

    if index != ' ' && index != '.' && index != '?' {
        changes.push(GitChange {
            path: file_path.to_string(),
            status: index.to_string(),
            staged: true,
            additions: None,
            deletions: None,
        });
    }

    if worktree != ' ' && worktree != '.' && worktree != '?' {
        changes.push(GitChange {
            path: file_path.to_string(),
            status: worktree.to_string(),
            staged: false,
            additions: None,
            deletions: None,
        });
    }
}

/// Run a git numstat command and merge the resulting additions/deletions
/// into the matching entries in `changes`.
fn merge_numstat(changes: &mut [GitChange], path: &Path, args: &[&str], staged: bool) {
    let output = std::process::Command::new("git")
        .args(
            std::iter::once("--no-optional-locks")
                .chain(args.iter().copied())
                .collect::<Vec<_>>(),
        )
        .current_dir(path)
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3 {
                    let adds = parts[0].parse::<i32>().ok();
                    let dels = parts[1].parse::<i32>().ok();
                    let fpath = parts[2];
                    for change in changes.iter_mut() {
                        if change.path == fpath && change.staged == staged {
                            change.additions = adds;
                            change.deletions = dels;
                        }
                    }
                }
            }
        }
    }
}
