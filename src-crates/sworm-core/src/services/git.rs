use crate::errors::ApiError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sworm_protocol::file_diff::{DiffSource, FileDiff, GitStatus};
use sworm_protocol::git::{
    CommitDetail, GitBrief, GitChange, GitQuickDiffData, GitSummary, GraphCommit, StashEntry,
};
use tracing::warn;

/// Hard limit on file content sent over IPC to prevent OOM on large files.
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024; // 2 MiB

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

/// Per-path cache slot. `generation` advances on every [`GitService::invalidate`]
/// so a summary computed before a mutation can never land in the cache after it:
/// `get_summary` reads the generation before running git and `store_summary`
/// drops the result if it moved meanwhile.
#[derive(Default)]
struct SummarySlot {
    generation: u64,
    cached: Option<CachedSummary>,
}

#[derive(Clone)]
struct CachedAheadBehind {
    head: String,
    upstream: String,
    value: (Option<i32>, Option<i32>),
}

fn resolve_ahead_behind_refs(path: &Path) -> Option<(String, String)> {
    let output = git_command(path)
        .args(["--no-optional-locks", "rev-parse", "HEAD", "@{upstream}"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut refs = text.lines().map(str::to_owned);
    Some((refs.next()?, refs.next()?))
}

fn parse_ahead_behind(stdout: &[u8]) -> Option<(Option<i32>, Option<i32>)> {
    let text = String::from_utf8_lossy(stdout);
    let mut parts = text.split_whitespace();
    let ahead = parts.next()?.parse::<i32>().ok();
    let behind = parts.next()?.parse::<i32>().ok();
    Some((ahead, behind))
}

fn parse_graph_commits(stdout: &[u8]) -> Vec<GraphCommit> {
    let lines: Vec<String> = String::from_utf8_lossy(stdout)
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

#[derive(Debug, thiserror::Error)]
pub enum DeleteBranchError {
    #[error("Branch not fully merged: {branch}")]
    BranchUnmerged { branch: String, message: String },

    #[error("{0}")]
    Git(String),
}

fn delete_branch_error(branch: &str, stderr: String) -> DeleteBranchError {
    if stderr.to_lowercase().contains("not fully merged") {
        DeleteBranchError::BranchUnmerged {
            branch: branch.to_string(),
            message: stderr,
        }
    } else {
        DeleteBranchError::Git(stderr)
    }
}

/// Git service using the system git CLI.
///
/// Holds an in-memory TTL cache of [`GitSummary`] values keyed by
/// project path. Mutating methods (`stage_*`, `commit`, `pull`, …) call
/// [`Self::invalidate`] so the next read picks up fresh state.
pub struct GitService {
    summary_cache: Mutex<HashMap<PathBuf, SummarySlot>>,
    ahead_behind_cache: Mutex<HashMap<PathBuf, CachedAheadBehind>>,
}

impl GitService {
    pub fn new() -> Self {
        Self {
            summary_cache: Mutex::new(HashMap::new()),
            ahead_behind_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn repo_root(path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(git_line(path, &["rev-parse", "--show-toplevel"])?).ok()
    }

    /// A fresh cached summary, or the slot generation a new computation
    /// must hand back to [`Self::store_summary`].
    fn cached_summary(&self, path: &Path) -> Result<GitSummary, u64> {
        let cache = self.summary_cache.lock();
        let Some(slot) = cache.get(path) else {
            return Err(0);
        };
        match &slot.cached {
            Some(entry) if entry.at.elapsed() < SUMMARY_CACHE_TTL => Ok(entry.summary.clone()),
            _ => Err(slot.generation),
        }
    }

    /// Store a summary computed under `generation`; silently dropped when an
    /// invalidation happened since, because the result predates that mutation.
    fn store_summary(&self, path: &Path, generation: u64, summary: &GitSummary) {
        let mut cache = self.summary_cache.lock();
        let slot = cache.entry(path.to_path_buf()).or_default();
        if slot.generation != generation {
            return;
        }
        slot.cached = Some(CachedSummary {
            at: Instant::now(),
            summary: summary.clone(),
        });
    }

    /// Drop any cached summary for `path` and advance its generation. Call
    /// after any mutation so the next [`Self::get_summary`] reflects the new
    /// state. Public so the git-dir watcher can invalidate external mutations.
    pub fn invalidate(&self, path: &Path) {
        let mut cache = self.summary_cache.lock();
        let slot = cache.entry(path.to_path_buf()).or_default();
        slot.generation += 1;
        slot.cached = None;
    }

    /// Evict any cached state for `path`. Called when a folder is closed.
    pub fn evict(&self, path: &Path) {
        self.summary_cache.lock().remove(path);
        self.ahead_behind_cache.lock().remove(path);
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

    /// Distinguish an ordinary non-repository folder from operational Git
    /// failures such as a missing cwd, unsafe ownership, or broken metadata.
    fn repository_state(&self, path: &Path) -> Result<bool, String> {
        let output = git_command(path)
            .args(["--no-optional-locks", "rev-parse", "--is-inside-work-tree"])
            .output()
            .map_err(|error| format!("Failed to inspect repository: {error}"))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim() == "true");
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") {
            Ok(false)
        } else {
            Err(git_failure("inspect repository", &output))
        }
    }

    /// Get the current branch name.
    pub fn current_branch(&self, path: &Path) -> Option<String> {
        git_line(path, &["branch", "--show-current"])
    }

    /// Attempt to detect the default base ref (e.g. origin/main).
    pub fn default_base_ref(&self, path: &Path) -> Option<String> {
        git_line(
            path,
            &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        )
        .or_else(|| {
            ["origin/main", "origin/master"]
                .into_iter()
                .find_map(|candidate| {
                    git_line(path, &["rev-parse", "--verify", candidate])
                        .map(|_| candidate.to_string())
                })
        })
    }

    /// Get full git summary for a project path.
    ///
    /// Cached for [`SUMMARY_CACHE_TTL`] to coalesce concurrent callers.
    /// Status failures remain errors: only Git's explicit "not a repository"
    /// result becomes a successful `is_repo: false` summary.
    pub fn get_summary(&self, path: &Path) -> Result<GitSummary, String> {
        let generation = match self.cached_summary(path) {
            Ok(cached) => return Ok(cached),
            Err(generation) => generation,
        };

        if !self.repository_state(path)? {
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
            self.store_summary(path, generation, &summary);
            return Ok(summary);
        }

        let (branch, base_ref, ahead_behind, changes) = std::thread::scope(|scope| {
            let branch = scope.spawn(|| self.current_branch(path));
            let base_ref = scope.spawn(|| self.default_base_ref(path));
            let ahead_behind = scope.spawn(|| self.ahead_behind(path));
            let changes = scope.spawn(|| self.get_changes(path));
            (
                branch.join().unwrap_or(None),
                base_ref.join().unwrap_or(None),
                ahead_behind.join().unwrap_or((None, None)),
                changes
                    .join()
                    .map_err(|_| "Git status worker panicked".to_string())
                    .and_then(|result| result),
            )
        });
        let changes = changes?;
        let (ahead, behind) = ahead_behind;
        let staged_count = changes.iter().filter(|change| change.staged).count() as i32;
        let unstaged_count = changes
            .iter()
            .filter(|change| !change.staged && change.status != "?")
            .count() as i32;
        let untracked_count = changes.iter().filter(|change| change.status == "?").count() as i32;

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
        self.store_summary(path, generation, &summary);
        Ok(summary)
    }

    /// Branch, distinct changed-path count and upstream ahead/behind from a
    /// single `git status` run, for surfaces that never show per-file changes.
    /// Uncached: Home asks once per card per visit. Error handling mirrors
    /// [`Self::get_summary`]: a missing directory fails, a plain one is `is_repo: false`.
    pub fn get_brief(&self, path: &Path) -> Result<GitBrief, String> {
        let status = status_records(path, true)?;
        Ok(match status {
            Some(status) => GitBrief {
                is_repo: true,
                branch: status.branch,
                changed: status.records.len() as u32,
                ahead: status.ahead,
                behind: status.behind,
            },
            None => GitBrief {
                is_repo: false,
                branch: None,
                changed: 0,
                ahead: None,
                behind: None,
            },
        })
    }

    /// Get changed files using porcelain v2 + numstat.
    fn get_changes(&self, path: &Path) -> Result<Vec<GitChange>, String> {
        let status = status_records(path, false)?.ok_or_else(|| {
            "Failed to read Git status: not a git repository or work tree".to_string()
        })?;
        let mut changes = Vec::new();
        for record in status.records {
            match record {
                StatusRecord::Tracked { path, xy } => push_status_entries(&mut changes, &path, &xy),
                StatusRecord::Untracked(path) => changes.push(GitChange {
                    path,
                    status: "?".to_string(),
                    staged: false,
                    additions: None,
                    deletions: None,
                }),
            }
        }

        // Numstat passes are conditional: skip the side that has no
        // entries from the porcelain pass; `git diff` with no changes
        // is still a process spawn we don't need to pay for.
        let has_unstaged = changes.iter().any(|c| !c.staged && c.status != "?");
        let has_staged = changes.iter().any(|c| c.staged);

        if has_unstaged {
            merge_numstat(&mut changes, path, &["diff", "--numstat", "-z"], false);
        }
        if has_staged {
            merge_numstat(
                &mut changes,
                path,
                &["diff", "--cached", "--numstat", "-z"],
                true,
            );
        }

        Ok(changes)
    }

    /// Get ahead/behind counts relative to the tracking branch.
    ///
    /// Walking the commit graph can be expensive on network filesystems.
    /// Resolve the two refs cheaply first and reuse the count while their
    /// object IDs remain unchanged; commits and fetches naturally miss.
    fn ahead_behind(&self, path: &Path) -> (Option<i32>, Option<i32>) {
        let Some((head, upstream)) = resolve_ahead_behind_refs(path) else {
            self.ahead_behind_cache.lock().remove(path);
            return (None, None);
        };

        if let Some(entry) = self.ahead_behind_cache.lock().get(path) {
            if entry.head == head && entry.upstream == upstream {
                return entry.value;
            }
        }

        let range = format!("{}...{}", head, upstream);
        let value = git_command(path)
            .args([
                "--no-optional-locks",
                "rev-list",
                "--left-right",
                "--count",
                &range,
            ])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| parse_ahead_behind(&output.stdout))
            .unwrap_or((None, None));

        self.ahead_behind_cache.lock().insert(
            path.to_path_buf(),
            CachedAheadBehind {
                head,
                upstream,
                value,
            },
        );
        value
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
        let output = git_command(path)
            .args([
                "--no-optional-locks",
                "log",
                "--all",
                "--topo-order",
                &format!("--max-count={}", limit),
                "--format=%H%n%h%n%P%n%an%n%aI%n%s%n%D",
            ])
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };

        if !output.status.success() {
            return Vec::new();
        }

        parse_graph_commits(&output.stdout)
    }

    /// Get linear commit history reachable from one branch ref.
    pub fn get_branch_commits(&self, path: &Path, branch: &str, limit: usize) -> Vec<GraphCommit> {
        let output = git_command(path)
            .args([
                "--no-optional-locks",
                "log",
                "--topo-order",
                &format!("--max-count={}", limit),
                "--format=%H%n%h%n%P%n%an%n%aI%n%s%n%D",
                branch,
                "--",
            ])
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };

        if !output.status.success() {
            return Vec::new();
        }

        parse_graph_commits(&output.stdout)
    }

    /// Get full commit detail (info + changed files with stats).
    pub fn get_commit_detail(&self, path: &Path, hash: &str) -> Option<CommitDetail> {
        // 1. Commit metadata
        // Use null-byte delimiters so %b (body) can contain newlines safely.
        let info = git_output(
            path,
            &[
                "show",
                "-s",
                "--format=%H%x00%h%x00%P%x00%an%x00%aI%x00%s%x00%b",
                hash,
            ],
            "read commit metadata",
        )
        .ok()?;
        let info_text = String::from_utf8_lossy(&info);
        let parts: Vec<&str> = info_text.splitn(7, '\0').collect();
        if parts.len() < 6 {
            return None;
        }

        let parents: Vec<String> = if parts[2].is_empty() {
            Vec::new()
        } else {
            parts[2].split(' ').map(|s| s.to_string()).collect()
        };

        // Merges compare against their first parent; roots have no old tree.
        let old = (!parents.is_empty()).then(|| format!("{hash}^"));
        let files = rev_entries(path, old.as_deref(), hash);

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

    // WRITE OPERATIONS //

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

    /// Discard all unstaged changes, removing untracked files before reverting
    /// tracked edits. Untracked files move to the trash unless `permanent`.
    pub fn discard_all(&self, path: &Path, permanent: bool) -> Result<(), ApiError> {
        ensure_no_conflicts(path, &[])?;
        self.remove_untracked(path, &[], permanent)?;
        self.run_mutate(path, &["checkout", "--", "."])
            .map_err(ApiError::Internal)
    }

    /// Discard changes for specific files or directories, removing untracked
    /// files before reverting tracked edits. All failures propagate.
    pub fn discard_files(
        &self,
        path: &Path,
        files: &[String],
        permanent: bool,
    ) -> Result<(), ApiError> {
        if files.is_empty() {
            return Ok(());
        }
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        ensure_no_conflicts(path, &refs)?;
        self.remove_untracked(path, &refs, permanent)?;

        let mut tracked_args = vec!["ls-files", "-z", "--"];
        tracked_args.extend(refs.iter().copied());
        let output =
            git_output(path, &tracked_args, "list tracked files").map_err(ApiError::Internal)?;
        let tracked = String::from_utf8_lossy(&output);
        let mut checkout_args = vec!["checkout", "--"];
        checkout_args.extend(refs.iter().copied().filter(|file| {
            let file = file.trim_end_matches('/');
            tracked.split('\0').any(|entry| {
                entry == file
                    || entry
                        .strip_prefix(file)
                        .is_some_and(|suffix| suffix.starts_with('/'))
            })
        }));
        if checkout_args.len() > 2 {
            self.run_mutate(path, &checkout_args)
                .map_err(ApiError::Internal)?;
        }
        Ok(())
    }

    fn remove_untracked(
        &self,
        path: &Path,
        pathspecs: &[&str],
        permanent: bool,
    ) -> Result<(), ApiError> {
        let files = untracked_files(path, pathspecs).map_err(ApiError::Internal)?;
        crate::services::removal::remove_paths(&files, permanent)?;
        prune_empty_dirs(path, &files);
        self.invalidate(path);
        Ok(())
    }

    /// Create a commit with the given message. Returns the new short hash.
    pub fn commit(&self, path: &Path, message: &str) -> Result<String, String> {
        self.run_mutate(path, &["commit", "-m", message])?;
        // Read back the new commit hash
        let output = git_output(path, &["rev-parse", "--short", "HEAD"], "read commit hash")?;
        Ok(String::from_utf8_lossy(&output).trim().to_string())
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
        let message = match git_output(
            path,
            &["log", "-1", "--format=%B", "HEAD"],
            "read last commit message",
        ) {
            Ok(output) => String::from_utf8_lossy(&output).trim_end().to_string(),
            Err(error) => {
                warn!(%error, "git log for undo_last_commit failed, proceeding with empty restore");
                String::new()
            }
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
        self.run_mutate(path, &["fetch", "--all", "--prune"])
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
        let output = git_command(path)
            .args(["--no-optional-locks", "stash", "list"])
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
        let text = run_git_capture(path, &["stash", "list", "--format=%gd%x00%gs%x00%aI"]);
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

                let files = parse_raw_numstat(&run_git_capture(
                    path,
                    &[
                        "stash",
                        "show",
                        "--raw",
                        "--numstat",
                        "-z",
                        "--include-untracked",
                        parts[0],
                    ],
                ));

                Some(StashEntry {
                    index,
                    message: parts[1].to_string(),
                    date: parts[2].to_string(),
                    files,
                })
            })
            .collect()
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

    // BRANCH OPERATIONS //
    //
    // Branch invariants for this block:
    //   * Every write goes through [`Self::run_mutate`] so the summary
    //     cache invalidates and the StatusBar reflects state inside one
    //     poll cycle.
    //   * Reads use a single `git for-each-ref` call to load local +
    //     remote branches in one shell-out; per-branch ahead/behind
    //     comes from a follow-up `rev-list --left-right --count` per
    //     branch with an upstream (errors silently fall to zeros so
    //     branches without upstreams still surface).
    //   * Paused-state detection ([`Self::branch_status`]) inspects
    //     `.git/rebase-merge`, `.git/rebase-apply`, and
    //     `.git/MERGE_HEAD` directly; do not parse `git status` output
    //     here, the directories are authoritative regardless of how
    //     the rebase or merge entered the paused state.

    /// List every local + remote-tracking branch in one shell-out.
    ///
    /// Format string tokens (each `%(…)` produces one tab-separated
    /// field; the order below matches the parsing in `parse_branch_row`):
    ///
    ///   `%(refname)`                  full ref (`refs/heads/main`)
    ///   `%(HEAD)`                     `*` for current branch
    ///   `%(objectname)`               full hash
    ///   `%(objectname:short)`         short hash
    ///   `%(upstream:short)`           upstream short name (empty if none)
    ///   `%(upstream:track,nobracket)` ahead / behind text
    ///   `%(authorname)`               commit's author name
    ///   `%(authordate:iso8601-strict)` ISO-8601 timestamp
    ///   `%(contents:subject)`         one-line subject
    pub fn list_branches(
        &self,
        path: &Path,
    ) -> Result<Vec<sworm_protocol::branch::BranchSummary>, String> {
        use sworm_protocol::branch::{BranchKind, BranchSummary};

        let format = "%(refname)\t%(HEAD)\t%(objectname)\t%(objectname:short)\t%(upstream:short)\t%(upstream:track,nobracket)\t%(authorname)\t%(authordate:iso8601-strict)\t%(contents:subject)";
        let output = git_command(path)
            .args([
                "--no-optional-locks",
                "for-each-ref",
                "--format",
                format,
                "refs/heads",
                "refs/remotes",
            ])
            .output()
            .map_err(|e| e.to_string())?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut out: Vec<BranchSummary> = Vec::new();
        for line in text.lines() {
            if let Some(row) = parse_branch_row(line) {
                // Skip the synthetic `refs/remotes/<remote>/HEAD` rows
                // that `for-each-ref` emits; they alias another ref
                // and would duplicate it in the list.
                if matches!(row.kind, BranchKind::Remote) && row.name.ends_with("/HEAD") {
                    continue;
                }
                let upstream = match row.kind {
                    BranchKind::Local => row.upstream,
                    BranchKind::Remote => Some(row.name.clone()),
                };
                out.push(BranchSummary {
                    name: row.name,
                    kind: row.kind,
                    is_current: matches!(row.kind, BranchKind::Local) && row.is_current,
                    upstream,
                    ahead: row.ahead,
                    behind: row.behind,
                    tip: row.tip,
                });
            }
        }
        Ok(out)
    }

    /// Inspect `.git/` sentinel dirs to determine paused state.
    ///
    /// `rebase-merge` is the interactive / new-style rebase dir;
    /// `rebase-apply` is the legacy `git am`-style rebase dir.
    /// `MERGE_HEAD` is a flat file written for the duration of a
    /// merge with conflicts. Use platform-safe `path.join(...)`
    /// rather than string concatenation.
    pub fn branch_status(&self, path: &Path) -> sworm_protocol::branch::BranchOpState {
        use sworm_protocol::branch::BranchOpState;
        let git_dir = resolve_git_dir(path);
        if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() {
            return BranchOpState::Rebasing;
        }
        if git_dir.join("MERGE_HEAD").exists() {
            return BranchOpState::Merging;
        }
        BranchOpState::Idle
    }

    /// Return true when the index, worktree, or untracked set has changes.
    pub fn is_worktree_dirty(&self, path: &Path) -> bool {
        self.get_changes(path)
            .is_ok_and(|changes| !changes.is_empty())
    }

    /// File metadata for `branch...HEAD` compare. Content stays lazy:
    /// the compare modal only renders status + path, and snapshots use
    /// the existing `git_show_file` path when a file is opened.
    pub fn diff_branch_against_head(&self, path: &Path, branch: &str) -> Vec<FileDiff> {
        rev_entries(path, Some("HEAD"), branch)
    }

    /// Switch to an existing branch. `git switch` refuses on a dirty
    /// tree; the frontend's `safeCheckout` enforces the dirty gate
    /// before this is reached.
    pub fn checkout_branch(&self, path: &Path, name: &str) -> Result<(), String> {
        self.run_mutate(path, &["switch", name])
    }

    /// Create a tracking local branch from a remote ref and switch to
    /// it (`git switch -c <local> --track <remote>`).
    pub fn checkout_remote_as_local(
        &self,
        path: &Path,
        remote_name: &str,
        local_name: &str,
    ) -> Result<(), String> {
        self.run_mutate(path, &["switch", "-c", local_name, "--track", remote_name])
    }

    /// Create a branch off `base`, optionally switching to it.
    pub fn create_branch(
        &self,
        path: &Path,
        name: &str,
        base: &str,
        checkout: bool,
    ) -> Result<(), String> {
        if checkout {
            self.run_mutate(path, &["switch", "-c", name, base])?;
        } else {
            self.run_mutate(path, &["branch", name, base])?;
        }
        Ok(())
    }

    /// Rename a local branch via `git branch -m <old> <new>`.
    pub fn rename_branch(&self, path: &Path, old: &str, new: &str) -> Result<(), String> {
        self.run_mutate(path, &["branch", "-m", old, new])
    }

    /// Delete a local branch. Without `force`, refuses unmerged
    /// branches as a typed error so the frontend can offer the
    /// force-delete fallback without parsing git stderr.
    pub fn delete_branch(
        &self,
        path: &Path,
        name: &str,
        force: bool,
    ) -> Result<(), DeleteBranchError> {
        let flag = if force { "-D" } else { "-d" };
        self.run_mutate(path, &["branch", flag, name])
            .map_err(|err| delete_branch_error(name, err))
    }

    /// Delete a remote branch by pushing a delete to its remote
    /// (`git push <remote> --delete <name>`).
    pub fn delete_remote_branch(
        &self,
        path: &Path,
        remote: &str,
        name: &str,
    ) -> Result<(), String> {
        self.run_mutate(path, &["push", remote, "--delete", name])
    }

    /// Set or change a branch's upstream tracking ref.
    pub fn set_upstream(&self, path: &Path, branch: &str, upstream: &str) -> Result<(), String> {
        let arg = format!("--set-upstream-to={}", upstream);
        self.run_mutate(path, &["branch", &arg, branch])
    }

    /// Fast-forward a branch to its upstream without checking it out
    /// (or `git pull --ff-only` for the current branch). Both forms
    /// reject non-fast-forward updates with a readable error.
    pub fn fast_forward(&self, path: &Path, branch: &str) -> Result<(), String> {
        let current = self.current_branch(path);
        if current.as_deref() == Some(branch) {
            return self.run_mutate(path, &["pull", "--ff-only"]);
        }
        let upstream = upstream_of(path, branch)
            .ok_or_else(|| format!("Branch '{}' has no upstream configured", branch))?;
        let (remote, remote_branch) = upstream
            .split_once('/')
            .ok_or_else(|| format!("Cannot parse upstream '{}'", upstream))?;
        let refspec = format!("{}:{}", remote_branch, branch);
        self.run_mutate(path, &["fetch", remote, &refspec])
    }

    /// Merge `source` into the current branch. `no_ff` forces a merge
    /// commit even when the merge could fast-forward.
    pub fn merge_into_current(&self, path: &Path, source: &str, no_ff: bool) -> Result<(), String> {
        let mut args = vec!["merge"];
        if no_ff {
            args.push("--no-ff");
        }
        args.push(source);
        self.run_mutate(path, &args)
    }

    /// Rebase the current branch onto `target`. Conflicts are surfaced
    /// through `branch_status` (paused-state directories), not by
    /// parsing this command's output.
    pub fn rebase_current_onto(&self, path: &Path, target: &str) -> Result<(), String> {
        self.run_mutate(path, &["rebase", target])
    }

    /// Continue a paused rebase after the user resolved conflicts.
    pub fn rebase_continue(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["rebase", "--continue"])
    }

    /// Conclude a paused merge with git's prepared MERGE_MSG.
    pub fn merge_continue(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["merge", "--continue"])
    }

    /// Skip the current commit during a paused rebase.
    pub fn rebase_skip(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["rebase", "--skip"])
    }

    /// Abort an in-flight rebase, restoring pre-rebase state.
    pub fn rebase_abort(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["rebase", "--abort"])
    }

    /// Abort an in-flight merge, restoring pre-merge state.
    pub fn merge_abort(&self, path: &Path) -> Result<(), String> {
        self.run_mutate(path, &["merge", "--abort"])
    }

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
        git_line(path, &["symbolic-ref", "refs/remotes/origin/HEAD"])
            .and_then(|value| {
                value
                    .strip_prefix("refs/remotes/origin/")
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "main".to_string())
    }

    // Diff lists carry metadata only; content is read one file at a time.

    /// Cheap index for the working tree: file list + metadata, no
    /// content. Pair with [`Self::get_diff_file_content`] to
    /// load each file lazily, avoiding a multi-megabyte payload to the
    /// frontend before the user has expanded any row.
    pub fn get_working_diff_index(&self, path: &Path, staged: bool) -> Vec<FileDiff> {
        let changes = self
            .get_summary(path)
            .map(|summary| summary.changes)
            .unwrap_or_default();
        filter_working_changes(changes, staged)
            .into_iter()
            .map(|change| {
                let lang = lang_from_path(&change.path).to_string();
                FileDiff {
                    path: change.path,
                    old_path: None,
                    status: GitStatus::from_code(&change.status),
                    lang,
                    additions: change.additions,
                    deletions: change.deletions,
                }
            })
            .collect()
    }

    /// Read one working-tree diff's content on the requested staged side.
    pub fn get_working_diff_file_content(
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
                fold_both_sides(old, BlobResult::Missing)
            }
            GitStatus::Added if staged => {
                // Staged addition: index has it, HEAD does not.
                let new = read_git_show_blob(path, &format!(":{}", file_path));
                fold_both_sides(BlobResult::Missing, new)
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

    /// Read one file's content without rediscovering the source's change list.
    pub fn get_diff_file_content(
        &self,
        path: &Path,
        source: &DiffSource,
        file_path: &str,
        old_path: Option<&str>,
        status: GitStatus,
    ) -> (Option<String>, Option<String>, bool) {
        match source {
            DiffSource::Working { staged } => {
                self.get_working_diff_file_content(path, file_path, status, *staged)
            }
            DiffSource::Commit { hash } => {
                // Root entries are additions, so no parent probe is needed.
                let old = if status == GitStatus::Added {
                    BlobResult::Missing
                } else {
                    read_git_show_blob(path, &format!("{hash}^:{}", old_path.unwrap_or(file_path)))
                };
                let new = if status == GitStatus::Deleted {
                    BlobResult::Missing
                } else {
                    read_git_show_blob(path, &format!("{hash}:{file_path}"))
                };
                fold_both_sides(old, new)
            }
            DiffSource::Stash { index } => {
                let stash_ref = format!("stash@{{{index}}}");
                let old = match status {
                    GitStatus::Added | GitStatus::Untracked => BlobResult::Missing,
                    _ => read_git_show_blob(
                        path,
                        &format!("{stash_ref}^:{}", old_path.unwrap_or(file_path)),
                    ),
                };
                let new = if status == GitStatus::Deleted {
                    BlobResult::Missing
                } else {
                    let blob = read_git_show_blob(path, &format!("{stash_ref}:{file_path}"));
                    if matches!(blob, BlobResult::Missing)
                        && matches!(status, GitStatus::Added | GitStatus::Untracked)
                    {
                        // Untracked files live on the stash's third parent.
                        read_git_show_blob(path, &format!("{stash_ref}^3:{file_path}"))
                    } else {
                        blob
                    }
                };
                fold_both_sides(old, new)
            }
        }
    }

    /// Resolve the current git identity for `path`. Prefers `user.email`,
    /// falls back to `user.name`. Returns `None` when neither is set.
    pub fn current_user_identity(&self, path: &Path) -> Option<String> {
        self.git_config_value(path, "user.email")
            .or_else(|| self.git_config_value(path, "user.name"))
    }

    fn git_config_value(&self, path: &Path, key: &str) -> Option<String> {
        git_line(path, &["config", "--get", key])
    }

    pub fn show_file(&self, repo: &Path, rev: &str, file: &str) -> Option<String> {
        git_show_raw_text(repo, &format!("{rev}:{file}"))
    }

    pub fn quick_diff_data(&self, repo: &Path, file: &str) -> GitQuickDiffData {
        let index_content = git_show_raw_text(repo, &format!(":{file}"));
        let head_content = git_show_raw_text(repo, &format!("HEAD:{file}"));
        // The dirty-diff editor is text-only, so mode-only changes do not count.
        let has_index_changes = index_content != head_content;
        GitQuickDiffData {
            index_content,
            head_content,
            has_index_changes,
        }
    }

    pub fn stage_file_content(
        &self,
        repo: &Path,
        file: &str,
        content: Option<&str>,
    ) -> Result<(), String> {
        match content {
            Some(content) => {
                let mode = git_file_mode(repo, file)?;
                let hash = git_hash_object(repo, file, content)?;
                self.run_mutate(
                    repo,
                    &["update-index", "--add", "--cacheinfo", &mode, &hash, file],
                )
            }
            None => self.run_mutate(repo, &["update-index", "--force-remove", "--", file]),
        }
    }
}

#[derive(Default)]
struct StatusV2 {
    branch: Option<String>,
    ahead: Option<u32>,
    behind: Option<u32>,
    records: Vec<StatusRecord>,
}

enum StatusRecord {
    Tracked { path: String, xy: String },
    Untracked(String),
}

fn status_records(path: &Path, with_branch: bool) -> Result<Option<StatusV2>, String> {
    let output = git_command(path)
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ])
        .arg(if with_branch {
            "--branch"
        } else {
            "--no-ahead-behind"
        })
        .output()
        .map_err(|error| format!("Failed to read Git status: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not a git repository") || stderr.contains("must be run in a work tree")
        {
            return Ok(None);
        }
        return Err(git_failure("read Git status", &output));
    }
    let mut status = StatusV2::default();
    let mut records = output.stdout.split(|byte| *byte == 0);
    while let Some(bytes) = records.next() {
        let record = String::from_utf8_lossy(bytes);
        match bytes.first() {
            Some(b'#') => {
                if let Some(head) = record.strip_prefix("# branch.head ") {
                    status.branch = (head != "(detached)").then(|| head.to_string());
                } else if let Some(ab) = record.strip_prefix("# branch.ab ") {
                    let (ahead, behind) = ab.split_once(' ').unwrap_or_default();
                    status.ahead = ahead.trim_start_matches('+').parse().ok();
                    status.behind = behind.trim_start_matches('-').parse().ok();
                }
            }
            Some(kind @ (b'1' | b'2' | b'u')) => {
                let fields = match kind {
                    b'1' => 9,
                    b'2' => 10,
                    _ => 11,
                };
                let mut parts = record.splitn(fields, ' ');
                parts.next();
                let xy = parts.next().unwrap_or_default();
                if let Some(path) = parts.nth(fields - 3) {
                    status.records.push(StatusRecord::Tracked {
                        path: path.to_string(),
                        xy: xy.to_string(),
                    });
                }
                if *kind == b'2' {
                    // Original rename/copy path is a separate NUL record, never a status row.
                    records.next();
                }
            }
            Some(b'?') => {
                if let Some(path) = record.strip_prefix("? ") {
                    status
                        .records
                        .push(StatusRecord::Untracked(path.to_string()));
                }
            }
            _ => {}
        }
    }
    Ok(Some(status))
}

fn rev_entries(path: &Path, old: Option<&str>, new: &str) -> Vec<FileDiff> {
    let text = match old {
        Some(old) => run_git_capture(
            path,
            &["diff", "--raw", "--numstat", "-z", "-M", "-C", old, new],
        ),
        None => run_git_capture(
            path,
            &[
                "diff-tree",
                "--root",
                "-r",
                "-z",
                "-M",
                "-C",
                "--raw",
                "--numstat",
                "--no-commit-id",
                new,
            ],
        ),
    };
    parse_raw_numstat(&text)
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

enum ShowOutput {
    Bytes(Vec<u8>),
    Oversized,
    Failed,
}

fn git_show_bounded(repo: &Path, args: &[&str]) -> ShowOutput {
    let mut child = match git_command(repo)
        .arg("--no-optional-locks")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return ShowOutput::Failed,
    };
    let stderr_thread = child.stderr.take().map(|mut stderr| {
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        })
    });
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(thread) = stderr_thread {
            let _ = thread.join();
        }
        return ShowOutput::Failed;
    };

    let mut bytes = Vec::new();
    let read_result = stdout
        .take(MAX_CONTENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes);
    if read_result.is_err() || bytes.len() > MAX_CONTENT_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        if let Some(thread) = stderr_thread {
            let _ = thread.join();
        }
        return if read_result.is_err() {
            ShowOutput::Failed
        } else {
            ShowOutput::Oversized
        };
    }

    let status = child.wait();
    if let Some(thread) = stderr_thread {
        let _ = thread.join();
    }
    match status {
        Ok(status) if status.success() => ShowOutput::Bytes(bytes),
        _ => ShowOutput::Failed,
    }
}

/// Read a blob via `git show` (supports `HEAD:path`, `:path`, `<ref>:path`).
fn read_git_show_blob(path: &Path, spec: &str) -> BlobResult {
    match git_show_bounded(path, &["show", "--no-textconv", spec]) {
        ShowOutput::Bytes(bytes) => classify_bytes(bytes),
        ShowOutput::Oversized => BlobResult::Oversized,
        ShowOutput::Failed => BlobResult::Missing,
    }
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
fn parse_numstat(text: &str) -> HashMap<String, (i32, i32)> {
    parse_numstat_fields(text.split('\0').filter(|field| !field.is_empty()))
}

fn parse_numstat_fields<'a>(fields: impl Iterator<Item = &'a str>) -> HashMap<String, (i32, i32)> {
    let mut map = HashMap::new();
    let mut fields = fields.peekable();

    while let Some(head) = fields.next() {
        // `head` is either "adds\tdels\tpath" or "adds\tdels\t" (rename prefix).
        let tab_parts: Vec<&str> = head.splitn(3, '\t').collect();
        if tab_parts.len() < 3 {
            continue;
        }
        let path = if tab_parts[2].is_empty() {
            let _old = fields.next();
            let Some(new_path) = fields.next() else {
                continue;
            };
            new_path.to_string()
        } else {
            tab_parts[2].to_string()
        };
        let Ok(adds) = tab_parts[0].parse::<i32>() else {
            continue;
        };
        let Ok(dels) = tab_parts[1].parse::<i32>() else {
            continue;
        };
        map.insert(path, (adds, dels));
    }
    map
}

fn name_status_entry<'a>(
    code: &str,
    fields: &mut impl Iterator<Item = &'a str>,
) -> Option<FileDiff> {
    let status = GitStatus::from_code(code.get(0..1).unwrap_or("M"));
    let (path, old_path) = match status {
        GitStatus::Renamed | GitStatus::Copied => {
            let old = fields.next()?;
            let new = fields.next()?;
            (new.to_string(), Some(old.to_string()))
        }
        _ => (fields.next()?.to_string(), None),
    };
    Some(FileDiff {
        lang: lang_from_path(&path).to_string(),
        path,
        old_path,
        status,
        additions: None,
        deletions: None,
    })
}

fn parse_raw_numstat(text: &str) -> Vec<FileDiff> {
    let mut entries = Vec::new();
    let mut fields = text
        .split('\0')
        .filter(|field| !field.is_empty())
        .peekable();
    while fields.peek().is_some_and(|field| field.starts_with(':')) {
        let header = fields.next().expect("peeked raw record");
        let code = header.rsplit(' ').next().unwrap_or("M");
        let Some(entry) = name_status_entry(code, &mut fields) else {
            break;
        };
        entries.push(entry);
    }
    let stats = parse_numstat_fields(fields);
    for entry in &mut entries {
        if let Some(&(additions, deletions)) = stats.get(&entry.path) {
            entry.additions = Some(additions);
            entry.deletions = Some(deletions);
        }
    }
    entries
}

/// Restrict working-tree changes to one side and dedupe by path.
/// Untracked files always live on the unstaged side.
fn filter_working_changes(changes: Vec<GitChange>, staged: bool) -> Vec<GitChange> {
    let mut seen = std::collections::HashSet::new();
    changes
        .into_iter()
        .filter(|change| {
            (if change.status == "?" {
                !staged
            } else {
                change.staged == staged
            }) && seen.insert(change.path.clone())
        })
        .collect()
}

fn git_show_raw_text(repo: &Path, spec: &str) -> Option<String> {
    // Quick-diff bases feed hunk staging/reverting, so they must be the
    // actual blob text, not display-only textconv output.
    match git_show_bounded(repo, &["show", "--no-textconv", spec]) {
        ShowOutput::Bytes(bytes) => String::from_utf8(bytes).ok(),
        ShowOutput::Oversized | ShowOutput::Failed => None,
    }
}

fn git_file_mode(repo: &Path, file: &str) -> Result<String, String> {
    for args in [
        ["ls-files", "-s", "--", file],
        ["ls-tree", "HEAD", "--", file],
    ] {
        if let Some(mode) = run_git_capture(repo, &args).split_whitespace().next() {
            return Ok(mode.to_string());
        }
    }
    Ok(git_worktree_file_mode(repo, file)?.unwrap_or_else(|| "100644".to_string()))
}

#[cfg(unix)]
fn git_worktree_file_mode(repo: &Path, file_path: &str) -> Result<Option<String>, String> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = match std::fs::symlink_metadata(repo.join(file_path)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Failed to read worktree file mode: {error}")),
    };
    let mode = if metadata.file_type().is_symlink() {
        "120000"
    } else if metadata.permissions().mode() & 0o111 != 0 {
        "100755"
    } else {
        "100644"
    };
    Ok(Some(mode.to_string()))
}

#[cfg(not(unix))]
fn git_worktree_file_mode(_repo: &Path, _file_path: &str) -> Result<Option<String>, String> {
    Ok(None)
}

fn git_hash_object(repo: &Path, file: &str, content: &str) -> Result<String, String> {
    let output = git_with_stdin(
        repo,
        &["hash-object", "--stdin", "-w", "--path", file],
        content.as_bytes(),
    )?;
    let hash = String::from_utf8_lossy(&output).trim().to_string();
    if hash.is_empty() {
        return Err("git hash-object returned an empty object id".to_string());
    }
    Ok(hash)
}

pub(crate) fn git_with_stdin(path: &Path, args: &[&str], input: &[u8]) -> Result<Vec<u8>, String> {
    let action = format!("run git {}", args.first().copied().unwrap_or_default());
    let mut child = git_command(path)
        .arg("--no-optional-locks")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to {action}: {error}"))?;
    let mut stdin = child.stdin.take().expect("piped git stdin");
    // Drain output while writing: check-ignore may emit more than a pipe can hold.
    let (output, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(input));
        let output = child.wait_with_output();
        (output, writer.join().expect("git stdin writer panicked"))
    });
    let output = output.map_err(|error| format!("Failed to {action}: {error}"))?;
    // check-ignore uses 1 for no matches; hash-object uses nonzero for real failures.
    if !output.status.success()
        && !(args.first() == Some(&"check-ignore") && output.status.code() == Some(1))
    {
        return Err(git_failure(&action, &output));
    }
    written.map_err(|error| format!("Failed to {action}: {error}"))?;
    Ok(output.stdout)
}

pub(crate) fn git_line(path: &Path, args: &[&str]) -> Option<String> {
    let output = git_output(path, args, "read Git output").ok()?;
    let value = String::from_utf8_lossy(&output).trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// Every Sworm git spawn: cwd = repo, never prompts on a terminal or opens an
/// editor, and speaks untranslated English so stderr matching stays stable.
fn git_command(path: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .current_dir(path)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("LANGUAGE", "C")
        .env("LC_ALL", "C.UTF-8");
    command
}

/// `checkout` cannot revert unmerged paths, so a discard spanning one would
/// remove untracked files and then fail. Refuse before touching anything.
fn ensure_no_conflicts(path: &Path, pathspecs: &[&str]) -> Result<(), ApiError> {
    let mut args = vec!["diff", "--name-only", "--diff-filter=U", "-z", "--"];
    args.extend_from_slice(pathspecs);
    let output = git_output(path, &args, "list conflicted files").map_err(ApiError::Internal)?;
    match output
        .split(|&byte| byte == 0)
        .find(|name| !name.is_empty())
    {
        Some(name) => Err(ApiError::InvalidArgument(format!(
            "Resolve conflicts before discarding: {}",
            String::from_utf8_lossy(name)
        ))),
        None => Ok(()),
    }
}

fn untracked_files(path: &Path, pathspecs: &[&str]) -> Result<Vec<PathBuf>, String> {
    let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
    args.extend_from_slice(pathspecs);
    let output = git_output(path, &args, "list untracked files")?;
    Ok(String::from_utf8_lossy(&output)
        .split('\0')
        .filter(|entry| !entry.is_empty() && !entry.ends_with('/'))
        .map(|entry| path.join(entry))
        .collect())
}

fn prune_empty_dirs(root: &Path, files: &[PathBuf]) {
    for file in files {
        for dir in file
            .ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(root) && *dir != root)
        {
            if std::fs::remove_dir(dir).is_err() {
                break;
            }
        }
    }
}

pub(crate) fn git_output(folder: &Path, args: &[&str], action: &str) -> Result<Vec<u8>, String> {
    let output = git_command(folder)
        .arg("--no-optional-locks")
        .args(args)
        .output()
        .map_err(|error| format!("Failed to {action}: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(git_failure(action, &output))
    }
}

pub(crate) fn git_failure(action: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("Failed to {action}: git exited with {}", output.status)
    } else {
        format!("Failed to {action}: {detail}")
    }
}
/// Run a git command and return stdout as a String (empty on failure).
fn run_git_capture(path: &Path, args: &[&str]) -> String {
    git_output(path, args, "read Git output")
        .map(|output| String::from_utf8_lossy(&output).into_owned())
        .unwrap_or_default()
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
    let body = run_git_capture(path, args);
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

/// One row from `git for-each-ref` parsed into the fields the
/// `BranchSummary` builder needs. Kept private to the branch-listing
/// path because the layout matches the format string used there.
struct BranchRefRow {
    name: String,
    kind: sworm_protocol::branch::BranchKind,
    is_current: bool,
    upstream: Option<String>,
    ahead: i32,
    behind: i32,
    tip: sworm_protocol::branch::BranchCommitRef,
}

/// Parse one `for-each-ref --format=…` line. Returns `None` for HEAD
/// detached entries, malformed rows, or refs we don't surface in the
/// Branches view.
fn parse_branch_row(line: &str) -> Option<BranchRefRow> {
    use sworm_protocol::branch::{BranchCommitRef, BranchKind};
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() < 9 {
        return None;
    }
    let refname = parts[0];
    let (name, kind) = if let Some(rest) = refname.strip_prefix("refs/heads/") {
        (rest.to_string(), BranchKind::Local)
    } else {
        let rest = refname.strip_prefix("refs/remotes/")?;
        (rest.to_string(), BranchKind::Remote)
    };
    let upstream = if parts[4].is_empty() {
        None
    } else {
        Some(parts[4].to_string())
    };
    let (ahead, behind) = parse_tracking_counts(parts[5]);
    Some(BranchRefRow {
        name,
        kind,
        is_current: parts[1] == "*",
        upstream,
        ahead,
        behind,
        tip: BranchCommitRef {
            hash: parts[2].to_string(),
            short_hash: parts[3].to_string(),
            author: parts[6].to_string(),
            date: parts[7].to_string(),
            // Subject can contain tabs in pathological cases; rejoin
            // the trailing fields so we don't truncate at the first
            // tab inside the subject.
            subject: parts[8..].join("\t"),
        },
    })
}

fn parse_tracking_counts(track: &str) -> (i32, i32) {
    let mut ahead = 0;
    let mut behind = 0;
    for part in track.split(',') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("ahead ") {
            ahead = value.parse().unwrap_or(0);
        } else if let Some(value) = trimmed.strip_prefix("behind ") {
            behind = value.parse().unwrap_or(0);
        }
    }
    (ahead, behind)
}

/// Look up the upstream short name for a local branch. Returns `None`
/// when the branch has no tracking ref.
fn upstream_of(path: &Path, branch: &str) -> Option<String> {
    let arg = format!("{}@{{upstream}}", branch);
    git_line(path, &["rev-parse", "--abbrev-ref", &arg])
}

/// Resolve the path to the repo's `.git` directory. Worktrees and
/// submodules can move it elsewhere; `git rev-parse --git-dir` is
/// the authoritative answer. Falls back to `<path>/.git` if the call
/// fails so paused-state detection still works on a basic repo.
fn resolve_git_dir(path: &Path) -> PathBuf {
    path.join(git_line(path, &["rev-parse", "--git-dir"]).unwrap_or_else(|| ".git".to_string()))
}

/// Run a mutating git command, returning `Ok(())` on success or
/// `Err(stderr)` on failure.
///
/// Mutators intentionally omit `--no-optional-locks`. Read-only git
/// calls use it to avoid optional index locks during polling; writes
/// need Git's normal locking so index and ref updates serialize safely.
fn run_git_mutate(path: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = git_command(path);
    command.args(args);
    // Mutations reach ssh, credential helpers, and pinentry. With no controlling
    // terminal those fail fast or use a GUI prompt instead of blocking on the
    // tty Sworm was launched from.
    #[cfg(unix)]
    // SAFETY: setsid is async-signal-safe and touches no parent state.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut command, || {
            libc::setsid();
            Ok(())
        });
    }
    let output = command.output().map_err(|e| e.to_string())?;

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
    let stats = parse_numstat(&run_git_capture(path, args));
    for change in changes.iter_mut().filter(|change| change.staged == staged) {
        if let Some(&(additions, deletions)) = stats.get(&change.path) {
            change.additions = Some(additions);
            change.deletions = Some(deletions);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use sworm_protocol::file_diff::GitStatus;

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
    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn empty_summary() -> GitSummary {
        GitSummary {
            is_repo: false,
            branch: None,
            base_ref: None,
            ahead: None,
            behind: None,
            changes: vec![],
            staged_count: 0,
            unstaged_count: 0,
            untracked_count: 0,
        }
    }

    /// A summary computed before an invalidation must never re-populate the
    /// cache after it, otherwise a slow pre-mutation `git status` resurrects
    /// stale state for a full TTL (the staged → unstaged → staged flicker).
    #[test]
    fn stale_summary_never_repopulates_cache() {
        let svc = GitService::new();
        let path = Path::new("/nonexistent/sworm-cache-test");
        let summary = empty_summary();

        svc.invalidate(path);
        let stale_generation = svc.cached_summary(path).unwrap_err();
        svc.invalidate(path);
        svc.store_summary(path, stale_generation, &summary);
        assert!(
            svc.cached_summary(path).is_err(),
            "stale store must be dropped"
        );

        let generation = svc.cached_summary(path).unwrap_err();
        svc.store_summary(path, generation, &summary);
        assert!(
            svc.cached_summary(path).is_ok(),
            "current store must be kept"
        );
    }

    #[test]
    fn summary_distinguishes_non_repo_from_git_failures() {
        let service = GitService::new();
        let plain = std::env::temp_dir().join(format!(
            "sworm-non-repo-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&plain).unwrap();
        assert!(!service.get_summary(&plain).unwrap().is_repo);

        let missing = plain.join("missing");
        assert!(
            service.get_summary(&missing).is_err(),
            "missing working directory must not masquerade as a non-repository"
        );

        let repo = plain.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        std::fs::write(repo.join(".git/index"), b"broken index").unwrap();
        assert!(
            service.get_summary(&repo).is_err(),
            "corrupt index must surface the git status failure"
        );

        std::fs::remove_dir_all(plain).ok();
    }

    #[test]
    fn brief_counts_distinct_paths_and_distinguishes_non_repo() {
        let service = GitService::new();
        let repo = temp_repo("brief");
        // Staged and unstaged edits to one path still count once.
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        std::fs::write(repo.join("a.txt"), "three\n").unwrap();
        std::fs::write(repo.join("new.txt"), "new\n").unwrap();

        let brief = service.get_brief(&repo).unwrap();
        assert!(brief.is_repo);
        assert_eq!(brief.branch.as_deref(), Some("main"));
        assert_eq!(brief.changed, 2);
        assert_eq!((brief.ahead, brief.behind), (None, None), "no upstream");

        let plain = repo.with_extension("plain");
        std::fs::create_dir_all(&plain).unwrap();
        assert!(!service.get_brief(&plain).unwrap().is_repo);
        assert!(
            service.get_brief(&plain.join("missing")).is_err(),
            "missing working directory must not masquerade as a non-repository"
        );

        std::fs::remove_dir_all(repo).ok();
        std::fs::remove_dir_all(plain).ok();
    }

    #[test]
    fn summary_preserves_spaces_in_tracked_paths() {
        let repo = temp_repo("spaces");
        std::fs::write(repo.join("file with space.txt"), "before").unwrap();
        git(&repo, &["add", "file with space.txt"]);
        git(&repo, &["commit", "-q", "-m", "add spaced path"]);
        std::fs::write(repo.join("file with space.txt"), "after").unwrap();

        let summary = GitService::new().get_summary(&repo).unwrap();
        assert!(
            summary
                .changes
                .iter()
                .any(|change| change.path == "file with space.txt"),
            "porcelain parser must preserve path spaces"
        );

        std::fs::remove_dir_all(repo).ok();
    }

    /// Regression test: a working-tree deletion must surface the prior
    /// content on the old side and `None` on the new side. Previous bug
    /// returned both sides empty, leaving the diff body blank.
    #[test]
    fn deletion_unstaged_returns_old_side_only() {
        let dir = std::env::temp_dir().join(format!(
            "sworm-del-test-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("foo.txt"), "alpha\nbeta\n").unwrap();
        git(&dir, &["add", "foo.txt"]);
        git(&dir, &["commit", "-q", "-m", "initial"]);
        std::fs::remove_file(dir.join("foo.txt")).unwrap();

        let svc = GitService::new();
        let (old, new, binary) =
            svc.get_working_diff_file_content(&dir, "foo.txt", GitStatus::Deleted, false);

        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(old.as_deref(), Some("alpha\nbeta\n"));
        assert_eq!(new, None);
        assert!(!binary);
    }

    /// Build a fixture repo with a current branch on a single commit
    /// and return its path. The caller is responsible for cleanup.
    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sworm-branch-{}-{}-{}",
            tag,
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        git(&dir, &["config", "user.email", "t@t.com"]);
        git(&dir, &["config", "user.name", "t"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn merge_continue_keeps_prepared_message() {
        let repo = temp_repo("merge-continue");
        git(&repo, &["switch", "-q", "-c", "feature"]);
        std::fs::write(repo.join("a.txt"), "feature\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "feature"]);
        git(&repo, &["switch", "-q", "main"]);
        std::fs::write(repo.join("a.txt"), "main\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "main"]);
        let merge = Command::new("git")
            .args(["merge", "feature"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!merge.status.success(), "merge must pause on a conflict");
        git(&repo, &["config", "core.editor", "false"]);
        std::fs::write(repo.join("a.txt"), "resolved\n").unwrap();
        git(&repo, &["add", "a.txt"]);

        GitService::new().merge_continue(&repo).unwrap();
        let message =
            git_output(&repo, &["log", "-1", "--format=%B"], "read merge message").unwrap();
        assert_eq!(
            String::from_utf8_lossy(&message).trim(),
            "Merge branch 'feature'"
        );
        git(&repo, &["rev-parse", "HEAD^2"]);
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn discard_files_reverts_tracked_and_removes_untracked() {
        let repo = temp_repo("discard-mixed");
        std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
        std::fs::create_dir_all(repo.join("new/deep")).unwrap();
        std::fs::write(repo.join("new/deep/x.txt"), "new\n").unwrap();

        GitService::new()
            .discard_files(&repo, &["a.txt".into(), "new".into()], true)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
            "one\n"
        );
        assert!(!repo.join("new").exists());
        assert!(git_output(&repo, &["status", "--porcelain"], "read status")
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn discard_files_reports_checkout_failure() {
        let repo = temp_repo("discard-checkout-failure");
        std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
        std::fs::write(repo.join(".git/index.lock"), "").unwrap();

        let error = GitService::new()
            .discard_files(&repo, &["a.txt".into()], true)
            .unwrap_err();
        assert!(error.to_string().contains("index.lock"), "{error}");
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn discard_refuses_conflicted_paths_before_removing_untracked() {
        let repo = temp_repo("discard-conflict");
        git(&repo, &["switch", "-q", "-c", "feature"]);
        std::fs::write(repo.join("a.txt"), "feature\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "feature"]);
        git(&repo, &["switch", "-q", "main"]);
        std::fs::write(repo.join("a.txt"), "main\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "main"]);
        let merge = Command::new("git")
            .args(["merge", "feature"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!merge.status.success(), "merge must pause on a conflict");
        std::fs::write(repo.join("u.txt"), "untracked\n").unwrap();

        let svc = GitService::new();
        assert!(matches!(
            svc.discard_all(&repo, true),
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(matches!(
            svc.discard_files(&repo, &["a.txt".into(), "u.txt".into()], true),
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(repo.join("u.txt").exists());

        svc.discard_files(&repo, &["u.txt".into()], true).unwrap();
        assert!(!repo.join("u.txt").exists());
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn push_without_credentials_fails_fast() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::{mpsc, Arc};

        let repo = temp_repo("push-no-credentials");
        git(&repo, &["config", "credential.helper", ""]);
        git(&repo, &["config", "core.askPass", ""]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = format!("http://{}/r.git", listener.local_addr().unwrap());
        git(&repo, &["remote", "add", "origin", &remote]);
        git(&repo, &["config", "branch.main.remote", "origin"]);
        git(&repo, &["config", "branch.main.merge", "refs/heads/main"]);

        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let server_stop = Arc::clone(&stop);
        let server = std::thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(1)))
                            .unwrap();
                        let mut request = [0; 4096];
                        assert!(stream.read(&mut request).unwrap() > 0);
                        stream
                            .write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"x\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                            .unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("accept credential request: {error}"),
                }
            }
        });
        let (sender, receiver) = mpsc::channel();
        let push_repo = repo.clone();
        let push = std::thread::spawn(move || {
            sender.send(GitService::new().push(&push_repo)).ok();
        });
        let result = receiver.recv_timeout(Duration::from_secs(20));
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap();
        std::fs::remove_dir_all(repo).unwrap();
        let error = result
            .expect("push must finish without credential prompts")
            .unwrap_err();
        push.join().unwrap();
        assert!(error.contains("terminal prompts disabled"), "{error}");
    }

    #[cfg(unix)]
    #[test]
    fn mutations_run_without_controlling_terminal() {
        use std::os::unix::fs::PermissionsExt;

        let repo = temp_repo("mutation-no-tty");
        let hooks = repo.join(".git/test-hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(
            &hook,
            "#!/bin/sh\nif (exec </dev/tty) 2>/dev/null; then echo \"has tty\" >&2; exit 1; fi\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        git(
            &repo,
            &["config", "core.hooksPath", hooks.to_str().unwrap()],
        );
        std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
        git(&repo, &["add", "a.txt"]);

        GitService::new().commit(&repo, "x").unwrap();
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn oversized_blob_is_reported_without_buffering() {
        let repo = temp_repo("bounded-blob");
        std::fs::write(repo.join("big.bin"), vec![b'x'; MAX_CONTENT_BYTES + 1]).unwrap();
        git(&repo, &["add", "big.bin"]);
        git(&repo, &["commit", "-q", "-m", "big"]);

        assert!(matches!(
            read_git_show_blob(&repo, "HEAD:big.bin"),
            BlobResult::Oversized
        ));
        let text = read_git_show_blob(&repo, "HEAD:a.txt");
        assert!(matches!(&text, BlobResult::Text(text) if text == "one\n"));
        assert!(matches!(
            read_git_show_blob(&repo, "HEAD:nope.txt"),
            BlobResult::Missing
        ));

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn staged_rename_with_spaces_keeps_numstat() {
        let repo = temp_repo("rename-stats");
        git(&repo, &["mv", "a.txt", "b c.txt"]);
        std::fs::write(repo.join("b c.txt"), "one\ntwo\n").unwrap();
        git(&repo, &["add", "-A"]);

        let summary = GitService::new().get_summary(&repo).unwrap();
        let change = summary
            .changes
            .iter()
            .find(|change| change.path == "b c.txt" && change.staged)
            .expect("staged rename");
        assert_eq!(change.additions, Some(1));
        assert_eq!(change.deletions, Some(0));

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn parse_raw_numstat_splits_raw_and_stat_records() {
        let text = ":100644 100644 aaaa bbbb M\0a.txt\0:100644 100644 aaaa bbbb R099\0old.txt\0new.txt\0:000000 100644 0000 cccc A\0new file.txt\x001\t1\ta.txt\x001\t0\t\0old.txt\0new.txt\x005\t0\tnew file.txt\0";
        let entries = parse_raw_numstat(text);

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].path, "a.txt");
        assert_eq!(entries[0].old_path, None);
        assert_eq!(entries[0].status, GitStatus::Modified);
        assert_eq!(entries[0].additions, Some(1));
        assert_eq!(entries[0].deletions, Some(1));
        assert_eq!(entries[1].path, "new.txt");
        assert_eq!(entries[1].old_path.as_deref(), Some("old.txt"));
        assert_eq!(entries[1].status, GitStatus::Renamed);
        assert_eq!(entries[1].additions, Some(1));
        assert_eq!(entries[1].deletions, Some(0));
        assert_eq!(entries[2].path, "new file.txt");
        assert_eq!(entries[2].status, GitStatus::Added);
        assert_eq!(entries[2].additions, Some(5));
    }

    #[test]
    fn stash_content_includes_untracked_with_stats() {
        let repo = temp_repo("stash");
        std::fs::write(repo.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.join("new file.txt"), "x\ny\n").unwrap();
        git(&repo, &["stash", "push", "-u", "-q"]);

        let service = GitService::new();
        let files = service.stash_list(&repo).remove(0).files;
        let tracked = files
            .iter()
            .find(|file| file.path == "a.txt")
            .expect("tracked stash entry");
        assert_eq!(tracked.additions, Some(1));
        assert_eq!(tracked.status, GitStatus::Modified);
        assert_eq!(
            service.get_diff_file_content(
                &repo,
                &DiffSource::Stash { index: 0 },
                "a.txt",
                None,
                GitStatus::Modified,
            ),
            (
                Some("one\n".to_string()),
                Some("one\ntwo\n".to_string()),
                false
            ),
        );

        let untracked = files
            .iter()
            .find(|file| file.path == "new file.txt")
            .expect("untracked stash entry");
        assert_eq!(untracked.status, GitStatus::Added);
        assert_eq!(untracked.additions, Some(2));
        assert_eq!(
            service.get_diff_file_content(
                &repo,
                &DiffSource::Stash { index: 0 },
                "new file.txt",
                None,
                GitStatus::Added,
            ),
            (None, Some("x\ny\n".to_string()), false),
        );

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn commit_content_reads_rename_old_side_and_root_add() {
        let repo = temp_repo("commit-content");
        // Amend the fixture's only commit so old.txt is a root addition.
        git(&repo, &["mv", "a.txt", "old.txt"]);
        std::fs::write(repo.join("old.txt"), "a\na\n").unwrap();
        git(&repo, &["add", "old.txt"]);
        git(&repo, &["commit", "-q", "--amend", "--no-edit"]);
        let root = git_line(&repo, &["rev-parse", "HEAD"]).unwrap();

        git(&repo, &["mv", "old.txt", "new.txt"]);
        // Retain two of three lines, safely above the 50% rename threshold.
        std::fs::write(repo.join("new.txt"), "a\na\nb\n").unwrap();
        git(&repo, &["add", "new.txt"]);
        git(&repo, &["commit", "-q", "-m", "rename with edit"]);
        let second = git_line(&repo, &["rev-parse", "HEAD"]).unwrap();

        let service = GitService::new();
        let detail = service.get_commit_detail(&repo, &second).unwrap();
        let renamed = detail
            .files
            .iter()
            .find(|file| file.path == "new.txt")
            .expect("renamed commit entry");
        assert_eq!(renamed.old_path.as_deref(), Some("old.txt"));
        assert_eq!(renamed.status, GitStatus::Renamed);
        assert_eq!(
            service.get_diff_file_content(
                &repo,
                &DiffSource::Commit { hash: second },
                "new.txt",
                Some("old.txt"),
                GitStatus::Renamed,
            ),
            (
                Some("a\na\n".to_string()),
                Some("a\na\nb\n".to_string()),
                false
            ),
        );
        assert_eq!(
            service.get_diff_file_content(
                &repo,
                &DiffSource::Commit { hash: root },
                "old.txt",
                None,
                GitStatus::Added,
            ),
            (None, Some("a\na\n".to_string()), false),
        );

        std::fs::remove_dir_all(repo).ok();
    }

    /// `list_branches` covers locals, remotes, the diverged remote
    /// case, and `current_branch` flagging for P0.T2.
    #[test]
    fn list_branches_returns_locals_remotes_with_ahead_behind() {
        let dir = temp_repo("list");

        // Two more local branches; one diverges from main.
        git(&dir, &["branch", "feature"]);
        git(&dir, &["branch", "diverged"]);

        // Set up a fake remote in a sibling bare repo, then push the
        // current main, plus a "diverged" branch that has one extra
        // commit upstream and one extra commit locally.
        let remote = dir.with_extension("remote.git");
        let _ = std::fs::remove_dir_all(&remote);
        std::fs::create_dir_all(&remote).unwrap();
        Command::new("git")
            .args(["init", "-q", "--bare"])
            .current_dir(&remote)
            .output()
            .unwrap();
        let remote_url = remote.to_string_lossy().into_owned();
        git(&dir, &["remote", "add", "origin", &remote_url]);
        git(&dir, &["push", "-q", "-u", "origin", "main"]);

        // Push the current "diverged" tip to origin, then add an
        // upstream-only commit (via a second clone), then add a
        // local-only commit so the branch is +1/-1.
        git(&dir, &["push", "-q", "-u", "origin", "diverged"]);
        let work2 = dir.with_extension("work2");
        let _ = std::fs::remove_dir_all(&work2);
        Command::new("git")
            .args(["clone", "-q", &remote_url, work2.to_str().unwrap()])
            .output()
            .unwrap();
        git(&work2, &["config", "user.email", "u@u.com"]);
        git(&work2, &["config", "user.name", "u"]);
        git(&work2, &["switch", "-q", "diverged"]);
        std::fs::write(work2.join("upstream.txt"), "u\n").unwrap();
        git(&work2, &["add", "upstream.txt"]);
        git(&work2, &["commit", "-q", "-m", "upstream"]);
        git(&work2, &["push", "-q"]);

        // Pull origin's new ref into our fixture's `refs/remotes` (no merge).
        git(&dir, &["fetch", "-q", "origin"]);

        // One local-only commit on `diverged`.
        git(&dir, &["switch", "-q", "diverged"]);
        std::fs::write(dir.join("local.txt"), "l\n").unwrap();
        git(&dir, &["add", "local.txt"]);
        git(&dir, &["commit", "-q", "-m", "local"]);
        git(&dir, &["switch", "-q", "main"]);

        let svc = GitService::new();
        let list = svc.list_branches(&dir).expect("list_branches succeeded");

        let by_name = |n: &str| {
            list.iter()
                .find(|b| b.name == n)
                .cloned()
                .unwrap_or_else(|| panic!("missing branch {} in {:?}", n, list))
        };

        assert!(by_name("main").is_current);
        assert!(!by_name("feature").is_current);
        assert!(!by_name("diverged").is_current);

        let diverged = by_name("diverged");
        assert_eq!(
            diverged.upstream.as_deref(),
            Some("origin/diverged"),
            "diverged should track origin/diverged"
        );
        assert_eq!(diverged.ahead, 1, "one local-only commit");
        assert_eq!(diverged.behind, 1, "one upstream-only commit");

        // origin/main exists as a remote-tracking row; the synthetic
        // origin/HEAD row is filtered out.
        assert!(list.iter().any(|b| b.name == "origin/main"));
        assert!(!list.iter().any(|b| b.name == "origin/HEAD"));

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&remote).ok();
        std::fs::remove_dir_all(&work2).ok();
    }

    #[test]
    fn ahead_behind_cache_tracks_ref_object_ids() {
        let dir = temp_repo("ahead-cache");
        let remote = dir.with_extension("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare"]);
        let remote_url = remote.to_string_lossy().into_owned();
        git(&dir, &["remote", "add", "origin", &remote_url]);
        git(&dir, &["push", "-q", "-u", "origin", "main"]);

        let svc = GitService::new();
        assert_eq!(svc.ahead_behind(&dir), (Some(0), Some(0)));
        assert_eq!(svc.ahead_behind(&dir), (Some(0), Some(0)));

        std::fs::write(dir.join("local.txt"), "local\n").unwrap();
        git(&dir, &["add", "local.txt"]);
        git(&dir, &["commit", "-q", "-m", "local"]);
        assert_eq!(svc.ahead_behind(&dir), (Some(1), Some(0)));

        git(&dir, &["push", "-q", "origin", "main"]);
        assert_eq!(svc.ahead_behind(&dir), (Some(0), Some(0)));

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&remote).ok();
    }

    #[test]
    fn delete_branch_returns_unmerged_variant() {
        let dir = temp_repo("delete-unmerged");
        git(&dir, &["branch", "topic"]);
        git(&dir, &["switch", "-q", "topic"]);
        std::fs::write(dir.join("topic.txt"), "topic\n").unwrap();
        git(&dir, &["add", "topic.txt"]);
        git(&dir, &["commit", "-q", "-m", "topic"]);
        git(&dir, &["switch", "-q", "main"]);

        let svc = GitService::new();
        let err = svc
            .delete_branch(&dir, "topic", false)
            .expect_err("unmerged branch should be rejected");

        match err {
            DeleteBranchError::BranchUnmerged { branch, message } => {
                assert_eq!(branch, "topic");
                assert!(message.to_lowercase().contains("not fully merged"));
            }
            DeleteBranchError::Git(message) => {
                panic!("expected BranchUnmerged, got git error: {}", message)
            }
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `branch_status` covers idle, mid-rebase, mid-merge for P0.T4.
    /// We synthesise the sentinel directories rather than driving git
    /// to a paused state; `branch_status` is a pure filesystem probe.
    #[test]
    fn branch_status_detects_paused_directories() {
        use sworm_protocol::branch::BranchOpState;
        let dir = temp_repo("status");
        let svc = GitService::new();

        assert_eq!(svc.branch_status(&dir), BranchOpState::Idle);

        let git_dir = dir.join(".git");
        std::fs::create_dir_all(git_dir.join("rebase-merge")).unwrap();
        assert_eq!(svc.branch_status(&dir), BranchOpState::Rebasing);
        std::fs::remove_dir_all(git_dir.join("rebase-merge")).unwrap();

        std::fs::create_dir_all(git_dir.join("rebase-apply")).unwrap();
        assert_eq!(svc.branch_status(&dir), BranchOpState::Rebasing);
        std::fs::remove_dir_all(git_dir.join("rebase-apply")).unwrap();

        std::fs::write(git_dir.join("MERGE_HEAD"), "deadbeef\n").unwrap();
        assert_eq!(svc.branch_status(&dir), BranchOpState::Merging);
        std::fs::remove_file(git_dir.join("MERGE_HEAD")).unwrap();

        assert_eq!(svc.branch_status(&dir), BranchOpState::Idle);

        std::fs::remove_dir_all(&dir).ok();
    }
}
