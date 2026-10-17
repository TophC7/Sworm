use crate::errors::ApiError;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use sworm_protocol::branch::BranchCommitRef;
use sworm_protocol::file_diff::{DiffSource, FileDiff, GitStatus};
use sworm_protocol::git::{
    CommitDetail, GitBrief, GitChange, GitCommitOptions, GitConflict, GitConflictKind,
    GitConflictSide, GitOperation, GitOperationKind, GitQuickDiffData, GitRemote,
    GitSequencerAction, GitStashScope, GitSummary, GraphCommit, StashEntry,
};
use tracing::warn;

mod worktrees;

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

/// Git service using the system git CLI.
///
/// Holds an in-memory TTL cache of [`GitSummary`] values keyed by
/// project path. Mutating methods (`stage`, `commit`, `pull`, …) call
/// [`Self::invalidate`] so the next read picks up fresh state.
pub struct GitService {
    summary_cache: Mutex<HashMap<PathBuf, SummarySlot>>,
    ahead_behind_cache: Mutex<HashMap<PathBuf, CachedAheadBehind>>,
    worktree_locks: Mutex<HashMap<PathBuf, std::sync::Weak<Mutex<()>>>>,
}

impl GitService {
    pub fn new() -> Self {
        Self {
            summary_cache: Mutex::new(HashMap::new()),
            ahead_behind_cache: Mutex::new(HashMap::new()),
            worktree_locks: Mutex::new(HashMap::new()),
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
    fn run_mutate(&self, path: &Path, args: &[&str]) -> Result<(), ApiError> {
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
            let summary = GitSummary::default();
            self.store_summary(path, generation, &summary);
            return Ok(summary);
        }

        let (base_ref, ahead_behind, status, repo_state) = std::thread::scope(|scope| {
            let base_ref = scope.spawn(|| self.default_base_ref(path));
            let ahead_behind = scope.spawn(|| self.ahead_behind(path));
            let status = scope.spawn(|| self.working_status(path));
            let repo_state = scope.spawn(|| {
                let git_dir = resolve_git_dir(path);
                (
                    read_operation(path, &git_dir),
                    read_message_template(&git_dir),
                )
            });
            (
                base_ref.join().unwrap_or(None),
                ahead_behind.join().unwrap_or((None, None)),
                status
                    .join()
                    .map_err(|_| "Git status worker panicked".to_string())
                    .and_then(|result| result),
                repo_state.join().unwrap_or_default(),
            )
        });
        let WorkingStatus {
            branch,
            detached,
            unborn,
            upstream,
            changes,
            conflicts,
        } = status?;
        let (operation, message_template) = repo_state;
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
            upstream,
            detached,
            unborn,
            operation,
            conflicts,
            message_template,
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

    /// Get branch state, changed files and conflicts using porcelain v2 + numstat.
    fn working_status(&self, path: &Path) -> Result<WorkingStatus, String> {
        let status = status_records(path, false)?.ok_or_else(|| {
            "Failed to read Git status: not a git repository or work tree".to_string()
        })?;
        let mut changes = Vec::new();
        let mut conflicts = Vec::new();
        for record in status.records {
            match record {
                StatusRecord::Tracked { path, xy } => push_status_entries(&mut changes, &path, &xy),
                StatusRecord::Unmerged { path, xy } => {
                    push_status_entries(&mut changes, &path, &xy);
                    let kind = match xy.as_str() {
                        "DD" => Some(GitConflictKind::BothDeleted),
                        "AU" => Some(GitConflictKind::AddedByUs),
                        "UD" => Some(GitConflictKind::DeletedByThem),
                        "UA" => Some(GitConflictKind::AddedByThem),
                        "DU" => Some(GitConflictKind::DeletedByUs),
                        "AA" => Some(GitConflictKind::BothAdded),
                        "UU" => Some(GitConflictKind::BothModified),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        conflicts.push(GitConflict { path, kind });
                    }
                }
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

        Ok(WorkingStatus {
            branch: status.branch,
            detached: status.detached,
            unborn: status.unborn,
            upstream: status.upstream,
            changes,
            conflicts,
        })
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

    /// Stage all changes or selected paths, guarding unresolved conflict markers.
    pub fn stage(
        &self,
        path: &Path,
        files: Option<&[String]>,
        allow_conflict_markers: bool,
    ) -> Result<(), ApiError> {
        if files.is_some_and(|files| files.is_empty()) {
            return Ok(());
        }
        let refs: Vec<&str> = files
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .collect();
        if !allow_conflict_markers {
            let paths: Vec<String> = unmerged_paths(path, &refs)?
                .into_iter()
                .filter(|file| {
                    std::fs::read_to_string(path.join(file))
                        .is_ok_and(|text| has_conflict_markers(&text))
                })
                .collect();
            if !paths.is_empty() {
                return Err(ApiError::ConflictMarkers { paths });
            }
        }
        let mut args = if files.is_none() {
            vec!["add", "-A"]
        } else {
            vec!["add", "--"]
        };
        args.extend(refs);
        self.run_mutate(path, &args)
    }

    /// Unstage all changes or selected paths, preserving the worktree.
    pub fn unstage(&self, path: &Path, files: Option<&[String]>) -> Result<(), ApiError> {
        if files.is_some_and(|files| files.is_empty()) {
            return Ok(());
        }
        let mut args = vec!["reset", "HEAD"];
        if let Some(files) = files {
            args.push("--");
            args.extend(files.iter().map(String::as_str));
        }
        self.run_mutate(path, &args)
    }

    /// Discard unstaged changes, trashing untracked files unless `permanent`.
    pub fn discard(
        &self,
        path: &Path,
        files: Option<&[String]>,
        permanent: bool,
    ) -> Result<(), ApiError> {
        let Some(files) = files else {
            ensure_no_conflicts(path, &[])?;
            self.remove_untracked(path, &[], permanent)?;
            return self.run_mutate(path, &["checkout", "--", "."]);
        };
        if files.is_empty() {
            return Ok(());
        }
        let refs: Vec<&str> = files.iter().map(String::as_str).collect();
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
            self.run_mutate(path, &checkout_args)?;
        }
        Ok(())
    }

    /// Append repo-root-relative paths to the root ignore file.
    pub fn ignore(&self, path: &Path, files: &[String]) -> Result<(), ApiError> {
        if files.is_empty() {
            return Ok(());
        }
        let root = Self::repo_root(path)
            .ok_or_else(|| ApiError::InvalidArgument("Not a git repository".to_string()))?;
        let ignore_file = root.join(".gitignore");
        let mut existing = match std::fs::read_to_string(&ignore_file) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(ApiError::Internal(error.to_string())),
        };
        if !existing.is_empty() && !existing.ends_with('\n') {
            existing.push('\n');
        }
        let entries: Vec<String> = files
            .iter()
            .map(|file| {
                let mut entry = file.replace('\\', "/").replace('[', "\\[");
                if entry.starts_with('#') || entry.starts_with('!') {
                    entry.insert(0, '\\');
                }
                entry
            })
            .collect();
        existing.push_str(&entries.join("\n"));
        existing.push('\n');
        std::fs::write(ignore_file, existing)
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        self.invalidate(path);
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

    /// Create or amend a commit. Returns its short hash.
    pub fn commit(
        &self,
        path: &Path,
        message: &str,
        options: &GitCommitOptions,
    ) -> Result<String, ApiError> {
        if options.all {
            self.stage(path, None, false)?;
        }
        let mut args = vec!["commit"];
        if options.amend {
            args.push("--amend");
        }
        if options.sign_off {
            args.push("--signoff");
        }
        if options.no_verify {
            args.push("--no-verify");
        }
        if !message.is_empty() {
            args.extend(["-m", message]);
        } else if options.amend {
            args.push("--no-edit");
        } else {
            return Err(ApiError::InvalidArgument(
                "Commit message cannot be empty".to_string(),
            ));
        }
        self.run_mutate(path, &args)?;
        let output = git_output(path, &["rev-parse", "--short", "HEAD"], "read commit hash")
            .map_err(ApiError::Internal)?;
        Ok(String::from_utf8_lossy(&output).trim().to_string())
    }

    /// Undo the last commit, preserving changes as staged, and return its
    /// message for the commit textarea. A root commit on a branch leaves it
    /// unborn with everything staged. Paused operations and detached root
    /// commits are rejected.
    pub fn undo_last_commit(&self, path: &Path) -> Result<String, ApiError> {
        if read_operation(path, &resolve_git_dir(path)).is_some() {
            return Err(ApiError::InvalidArgument(
                "Finish the paused Git operation before undoing the last commit".to_string(),
            ));
        }
        let head = git_output(path, &["rev-parse", "--verify", "HEAD"], "resolve HEAD")
            .map_err(ApiError::Internal)?;
        let head = String::from_utf8_lossy(&head).trim().to_string();

        // Snapshot the message BEFORE resetting. `%B` is the full raw
        // body (subject + body), which matches what the user originally
        // typed into the textarea. `trim_end` removes both `\n` and
        // `\r\n` so the textarea doesn't inherit a stray CR on
        // CRLF-flavoured repos.
        let message = match git_output(
            path,
            &["log", "-1", "--format=%B", &head],
            "read last commit message",
        ) {
            Ok(output) => String::from_utf8_lossy(&output).trim_end().to_string(),
            Err(error) => {
                warn!(%error, "git log for undo_last_commit failed, proceeding with empty restore");
                String::new()
            }
        };

        let commit = git_output(path, &["cat-file", "commit", &head], "read last commit")
            .map_err(ApiError::Internal)?;
        // The raw object keeps its parent headers even when a shallow clone hides the parent,
        // so only a true root commit reaches the ref deletion below.
        let has_parent = String::from_utf8_lossy(&commit)
            .lines()
            .take_while(|line| !line.is_empty())
            .any(|line| line.starts_with("parent "));
        if has_parent {
            self.run_mutate(path, &["reset", "--soft", "HEAD~1"])?;
        } else {
            let branch = self.current_branch(path).ok_or_else(|| {
                ApiError::InvalidArgument(
                    "Cannot undo a root commit on a detached HEAD".to_string(),
                )
            })?;
            let ref_name = format!("refs/heads/{branch}");
            // Root commit: drop the branch ref; the index keeps the files staged, matching the soft reset.
            self.run_mutate(path, &["update-ref", "-d", &ref_name, &head])?;
        }
        Ok(message)
    }

    fn checked_out_branch(&self, path: &Path, branch: Option<&str>) -> Result<String, ApiError> {
        branch
            .map(str::to_owned)
            .or_else(|| self.current_branch(path))
            .ok_or_else(|| ApiError::InvalidArgument("No branch is checked out".to_string()))
    }

    /// Push an explicitly configured local branch to its upstream.
    pub fn push(&self, path: &Path, branch: Option<&str>, force: bool) -> Result<(), ApiError> {
        let branch = self.checked_out_branch(path, branch)?;
        let (remote, merge) =
            upstream_config(path, &branch).ok_or_else(|| ApiError::NoUpstream {
                branch: branch.clone(),
            })?;
        let refspec = format!("refs/heads/{branch}:{merge}");
        let mut args = vec!["push"];
        if force {
            args.push("--force-with-lease");
        }
        args.extend([remote.as_str(), refspec.as_str()]);
        self.run_mutate(path, &args)
    }

    /// Pull the current branch's explicit upstream, honoring pull config unless rebasing.
    pub fn pull(&self, path: &Path, rebase: bool) -> Result<(), ApiError> {
        let branch = self.checked_out_branch(path, None)?;
        let (remote, merge) =
            upstream_config(path, &branch).ok_or_else(|| ApiError::NoUpstream { branch })?;
        let mut args = vec!["pull"];
        if rebase {
            args.push("--rebase");
        }
        args.extend([remote.as_str(), merge.as_str()]);
        self.run_mutate(path, &args)
    }

    /// Fetch one remote or all remotes, pruning stale tracking refs.
    pub fn fetch(&self, path: &Path, remote: Option<&str>) -> Result<(), ApiError> {
        match remote {
            Some(remote) => self.run_mutate(path, &["fetch", "--prune", remote]),
            None => self.run_mutate(path, &["fetch", "--all", "--prune"]),
        }
    }

    /// Publish a local branch and establish its upstream.
    pub fn publish(
        &self,
        path: &Path,
        branch: Option<&str>,
        remote: Option<&str>,
    ) -> Result<(), ApiError> {
        let branch = self.checked_out_branch(path, branch)?;
        let remote = match remote
            .map(str::to_owned)
            .or_else(|| git_line(path, &["config", "--get", "remote.pushDefault"]))
        {
            Some(remote) => remote,
            None => {
                let output =
                    git_output(path, &["remote"], "list remotes").map_err(ApiError::Internal)?;
                let text = String::from_utf8_lossy(&output);
                let remotes: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
                match remotes.as_slice() {
                    [] => {
                        return Err(ApiError::InvalidArgument(
                            "Add a remote before publishing".to_string(),
                        ))
                    }
                    [remote] => (*remote).to_string(),
                    _ if remotes.contains(&"origin") => "origin".to_string(),
                    _ => {
                        return Err(ApiError::InvalidArgument(
                            "Choose a remote to publish to".to_string(),
                        ))
                    }
                }
            }
        };
        let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
        self.run_mutate(path, &["push", "-u", &remote, &refspec])
    }

    /// Pull first, then push only when local commits remain ahead.
    pub fn sync(&self, path: &Path, rebase: bool) -> Result<(), ApiError> {
        self.pull(path, rebase)?;
        let ahead = git_line(path, &["rev-list", "--count", "@{upstream}..HEAD"])
            .and_then(|count| count.parse::<u32>().ok())
            .ok_or_else(|| {
                ApiError::Internal("Failed to count commits ahead of upstream".to_string())
            })?;
        if ahead > 0 {
            self.push(path, None, false)?;
        }
        Ok(())
    }

    /// Stash all, staged, or selected changes.
    pub fn stash_push(
        &self,
        path: &Path,
        message: Option<&str>,
        scope: &GitStashScope,
    ) -> Result<(), ApiError> {
        let mut args = vec!["stash", "push"];
        match scope {
            GitStashScope::All { include_untracked } => {
                if *include_untracked {
                    args.push("--include-untracked");
                }
            }
            GitStashScope::Staged => args.push("--staged"),
            GitStashScope::Paths { paths } => {
                if paths.is_empty() {
                    return Err(ApiError::InvalidArgument("No paths to stash".to_string()));
                }
                args.push("--include-untracked");
            }
        }
        if let Some(message) = message {
            args.extend(["-m", message]);
        }
        if let GitStashScope::Paths { paths } = scope {
            args.push("--");
            args.extend(paths.iter().map(String::as_str));
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

    /// Apply a stash entry, optionally dropping it after success.
    pub fn stash_apply(&self, path: &Path, index: usize, pop: bool) -> Result<(), ApiError> {
        let stash_ref = format!("stash@{{{index}}}");
        self.run_mutate(
            path,
            &["stash", if pop { "pop" } else { "apply" }, &stash_ref],
        )
    }

    /// Drop a stash entry by index without applying it.
    pub fn stash_drop(&self, path: &Path, index: usize) -> Result<(), ApiError> {
        let stash_ref = format!("stash@{{{}}}", index);
        self.run_mutate(path, &["stash", "drop", &stash_ref])
    }

    pub fn stash_clear(&self, path: &Path) -> Result<(), ApiError> {
        self.run_mutate(path, &["stash", "clear"])
    }

    pub fn stash_branch(&self, path: &Path, index: usize, name: &str) -> Result<(), ApiError> {
        let stash_ref = format!("stash@{{{index}}}");
        self.run_mutate(path, &["stash", "branch", name, &stash_ref])
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

        let inventory = match self.list_worktrees(path) {
            Ok(inventory) => Some(inventory),
            Err(error) => {
                tracing::warn!(folder = %path.display(), %error, "worktree inventory unavailable; branch list omits worktree paths");
                None
            }
        };
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
                    kind: row.kind,
                    is_current: matches!(row.kind, BranchKind::Local) && row.is_current,
                    worktree_path: if matches!(row.kind, BranchKind::Local) {
                        inventory.as_ref().and_then(|inventory| {
                            inventory
                                .worktrees
                                .iter()
                                .find(|worktree| {
                                    worktree.branch.as_deref() == Some(row.name.as_str())
                                })
                                .map(|worktree| worktree.path.clone())
                        })
                    } else {
                        None
                    },
                    name: row.name,
                    upstream,
                    ahead: row.ahead,
                    behind: row.behind,
                    tip: row.tip,
                });
            }
        }
        Ok(out)
    }

    /// Configured remotes with their URLs, in `git remote` order.
    pub fn list_remotes(&self, path: &Path) -> Result<Vec<GitRemote>, ApiError> {
        let (output, push_default) = std::thread::scope(|scope| {
            let push_default =
                scope.spawn(|| git_line(path, &["config", "--get", "remote.pushDefault"]));
            let output = git_output(path, &["remote", "-v"], "list remotes");
            (output, push_default.join().unwrap_or_default())
        });
        let output = output.map_err(ApiError::Internal)?;
        let text = String::from_utf8_lossy(&output);
        let mut remotes: Vec<GitRemote> = Vec::new();
        for line in text.lines() {
            let Some((name, rest)) = line.split_once('\t') else {
                continue;
            };
            let Some((url, kind)) = rest.rsplit_once(' ') else {
                continue;
            };
            let index = match remotes.iter().position(|remote| remote.name == name) {
                Some(index) => index,
                None => {
                    remotes.push(GitRemote {
                        name: name.to_string(),
                        fetch_url: None,
                        push_url: None,
                        push_default: push_default.as_deref() == Some(name),
                    });
                    remotes.len() - 1
                }
            };
            let remote = &mut remotes[index];
            match kind {
                "(fetch)" if remote.fetch_url.is_none() => remote.fetch_url = Some(url.to_string()),
                "(push)" if remote.push_url.is_none() => remote.push_url = Some(url.to_string()),
                _ => {}
            }
        }
        Ok(remotes)
    }

    /// Return true when the index, worktree, or untracked set has changes.
    pub fn is_worktree_dirty(&self, path: &Path) -> bool {
        self.working_status(path)
            .is_ok_and(|status| !status.changes.is_empty())
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
    pub fn checkout_branch(&self, path: &Path, name: &str) -> Result<(), ApiError> {
        self.run_mutate(path, &["switch", name])
    }

    pub fn checkout_detached(&self, path: &Path, rev: &str) -> Result<(), ApiError> {
        self.run_mutate(path, &["switch", "--detach", rev])
    }

    /// Create a tracking local branch from a remote ref and switch to
    /// it (`git switch -c <local> --track <remote>`).
    pub fn checkout_remote_as_local(
        &self,
        path: &Path,
        remote_name: &str,
        local_name: &str,
    ) -> Result<(), ApiError> {
        self.run_mutate(path, &["switch", "-c", local_name, "--track", remote_name])
    }

    /// Create a branch off `base`, optionally switching to it.
    pub fn create_branch(
        &self,
        path: &Path,
        name: &str,
        base: &str,
        checkout: bool,
    ) -> Result<(), ApiError> {
        if checkout {
            self.run_mutate(path, &["switch", "-c", name, base])?;
        } else {
            self.run_mutate(path, &["branch", name, base])?;
        }
        Ok(())
    }

    /// Rename a local branch via `git branch -m <old> <new>`.
    pub fn rename_branch(&self, path: &Path, old: &str, new: &str) -> Result<(), ApiError> {
        self.run_mutate(path, &["branch", "-m", old, new])
    }

    /// Delete a local branch. Without `force`, refuses unmerged
    /// branches as a typed error so the frontend can offer the
    /// force-delete fallback without parsing git stderr.
    pub fn delete_branch(&self, path: &Path, name: &str, force: bool) -> Result<(), ApiError> {
        let flag = if force { "-D" } else { "-d" };
        self.run_mutate(path, &["branch", flag, name])
    }

    /// Delete a remote branch by pushing a delete to its remote
    /// (`git push <remote> --delete <name>`).
    pub fn delete_remote_branch(
        &self,
        path: &Path,
        remote: &str,
        name: &str,
    ) -> Result<(), ApiError> {
        self.run_mutate(path, &["push", remote, "--delete", name])
    }

    /// Set or change a branch's upstream tracking ref.
    pub fn set_upstream(&self, path: &Path, branch: &str, upstream: &str) -> Result<(), ApiError> {
        let arg = format!("--set-upstream-to={}", upstream);
        self.run_mutate(path, &["branch", &arg, branch])
    }

    pub fn unset_upstream(&self, path: &Path, branch: &str) -> Result<(), ApiError> {
        self.run_mutate(path, &["branch", "--unset-upstream", branch])
    }

    /// Fast-forward a branch to its upstream without checking it out
    /// (or `git pull --ff-only` for the current branch). Both forms
    /// reject non-fast-forward updates with a readable error.
    pub fn fast_forward(&self, path: &Path, branch: &str) -> Result<(), ApiError> {
        let (remote, merge) =
            upstream_config(path, branch).ok_or_else(|| ApiError::NoUpstream {
                branch: branch.to_string(),
            })?;
        if self.current_branch(path).as_deref() == Some(branch) {
            return self.run_mutate(path, &["pull", "--ff-only", &remote, &merge]);
        }
        let refspec = format!("{merge}:refs/heads/{branch}");
        self.run_mutate(path, &["fetch", &remote, &refspec])
    }

    /// Merge `source` into the current branch. `no_ff` forces a merge
    /// commit even when the merge could fast-forward.
    pub fn merge_into_current(
        &self,
        path: &Path,
        source: &str,
        no_ff: bool,
    ) -> Result<(), ApiError> {
        let mut args = vec!["merge"];
        if no_ff {
            args.push("--no-ff");
        }
        args.push(source);
        self.run_mutate(path, &args)
    }

    /// Rebase the current branch onto `target`, surfacing typed conflicts.
    pub fn rebase_current_onto(&self, path: &Path, target: &str) -> Result<(), ApiError> {
        self.run_mutate(path, &["rebase", target])
    }

    /// Continue, skip, or abort a paused operation.
    pub fn sequencer(
        &self,
        path: &Path,
        operation: GitOperationKind,
        action: &GitSequencerAction,
    ) -> Result<(), ApiError> {
        let command = match operation {
            GitOperationKind::Merge => "merge",
            GitOperationKind::Rebase => "rebase",
            GitOperationKind::CherryPick => "cherry-pick",
            GitOperationKind::Revert => "revert",
        };
        match action {
            GitSequencerAction::Abort => self.run_mutate(path, &[command, "--abort"]),
            GitSequencerAction::Skip => {
                if operation == GitOperationKind::Merge {
                    return Err(ApiError::InvalidArgument(
                        "A merge cannot be skipped".to_string(),
                    ));
                }
                self.run_mutate(path, &[command, "--skip"])
            }
            GitSequencerAction::Continue { message } => {
                if let Some(message) = message
                    .as_deref()
                    .map(str::trim)
                    .filter(|message| !message.is_empty())
                {
                    self.commit(path, message, &GitCommitOptions::default())?;
                    if !read_operation(path, &resolve_git_dir(path))
                        .is_some_and(|state| state.kind == operation)
                    {
                        return Ok(());
                    }
                }
                self.run_mutate(path, &[command, "--continue"])
            }
        }
    }

    /// Resolve conflicts using the chosen index stage, including side deletions.
    pub fn take_conflict_side(
        &self,
        path: &Path,
        files: &[String],
        side: GitConflictSide,
    ) -> Result<(), ApiError> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["ls-files", "-u", "-z", "--"];
        args.extend(files.iter().map(String::as_str));
        let output = git_output(path, &args, "list conflict stages").map_err(ApiError::Internal)?;
        let text = String::from_utf8_lossy(&output);
        let mut unmerged = HashSet::new();
        let mut chosen = HashSet::new();
        let stage = if side == GitConflictSide::Ours {
            "2"
        } else {
            "3"
        };
        for record in text.split('\0') {
            if let Some((metadata, file)) = record.split_once('\t') {
                unmerged.insert(file);
                if metadata.split_whitespace().nth(2) == Some(stage) {
                    chosen.insert(file);
                }
            }
        }
        let invalid: Vec<&str> = files
            .iter()
            .map(String::as_str)
            .filter(|file| !unmerged.contains(file))
            .collect();
        if !invalid.is_empty() {
            return Err(ApiError::InvalidArgument(format!(
                "Not conflicted: {}",
                invalid.join(", ")
            )));
        }
        let (present, deleted): (Vec<&str>, Vec<&str>) = files
            .iter()
            .map(String::as_str)
            .partition(|file| chosen.contains(file));
        if !present.is_empty() {
            let flag = if side == GitConflictSide::Ours {
                "--ours"
            } else {
                "--theirs"
            };
            let mut checkout = vec!["checkout", flag, "--"];
            checkout.extend_from_slice(&present);
            self.run_mutate(path, &checkout)?;
            let mut add = vec!["add", "--"];
            add.extend(present);
            self.run_mutate(path, &add)?;
        }
        if !deleted.is_empty() {
            // A deletion side can leave modified conflict content in the worktree.
            let mut remove = vec!["rm", "--quiet", "--force", "--"];
            remove.extend(deleted);
            self.run_mutate(path, &remove)?;
        }
        Ok(())
    }

    pub fn delete_conflicted(&self, path: &Path, files: &[String]) -> Result<(), ApiError> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["rm", "--quiet", "--"];
        args.extend(files.iter().map(String::as_str));
        self.run_mutate(path, &args)
    }

    /// Initialize a new git repository. Repo identity changes (now a
    /// repo), so [`Self::run_mutate`]'s invalidation drops any stale
    /// "not a repo" summary on next read.
    pub fn init(&self, path: &Path) -> Result<(), ApiError> {
        self.run_mutate(path, &["init"])
    }

    /// Clone a repository into the given directory (in-place, no subfolder).
    ///
    /// Uses `git clone <url> .` for empty dirs, or init + remote + fetch +
    /// checkout for dirs with existing content. Each step routes through
    /// [`Self::run_mutate`] so the summary cache is invalidated even on
    /// partial completion.
    pub fn clone_in_place(&self, path: &Path, url: &str) -> Result<(), ApiError> {
        let has_content = path
            .read_dir()
            .map_err(|e| ApiError::Internal(format!("Cannot read directory: {}", e)))?
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
    ) -> Result<(), ApiError> {
        match content {
            Some(content) => {
                let mode = git_file_mode(repo, file).map_err(ApiError::Internal)?;
                let hash = git_hash_object(repo, file, content).map_err(ApiError::Internal)?;
                self.run_mutate(
                    repo,
                    &["update-index", "--add", "--cacheinfo", &mode, &hash, file],
                )
            }
            None => self.run_mutate(repo, &["update-index", "--force-remove", "--", file]),
        }
    }
}

struct WorkingStatus {
    branch: Option<String>,
    detached: bool,
    unborn: bool,
    upstream: Option<String>,
    changes: Vec<GitChange>,
    conflicts: Vec<GitConflict>,
}

#[derive(Default)]
struct StatusV2 {
    branch: Option<String>,
    upstream: Option<String>,
    detached: bool,
    unborn: bool,
    ahead: Option<u32>,
    behind: Option<u32>,
    records: Vec<StatusRecord>,
}

enum StatusRecord {
    Tracked { path: String, xy: String },
    Unmerged { path: String, xy: String },
    Untracked(String),
}

fn status_records(path: &Path, ahead_behind: bool) -> Result<Option<StatusV2>, String> {
    let mut command = git_command(path);
    command.args([
        "--no-optional-locks",
        "status",
        "--porcelain=v2",
        "-z",
        "--untracked-files=all",
        "--branch",
    ]);
    if !ahead_behind {
        command.arg("--no-ahead-behind");
    }
    let output = command
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
                    status.detached = head == "(detached)";
                    status.branch = (!status.detached).then(|| head.to_string());
                } else if record == "# branch.oid (initial)" {
                    status.unborn = true;
                } else if let Some(upstream) = record.strip_prefix("# branch.upstream ") {
                    status.upstream = Some(upstream.to_string());
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
                    let path = path.to_string();
                    let xy = xy.to_string();
                    status.records.push(if *kind == b'u' {
                        StatusRecord::Unmerged { path, xy }
                    } else {
                        StatusRecord::Tracked { path, xy }
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
    match unmerged_paths(path, pathspecs)?.first() {
        Some(name) => Err(ApiError::InvalidArgument(format!(
            "Resolve conflicts before discarding: {name}"
        ))),
        None => Ok(()),
    }
}

fn unmerged_paths(path: &Path, pathspecs: &[&str]) -> Result<Vec<String>, ApiError> {
    let mut args = vec!["diff", "--name-only", "--diff-filter=U", "-z", "--"];
    args.extend_from_slice(pathspecs);
    let output = git_output(path, &args, "list conflicted files").map_err(ApiError::Internal)?;
    Ok(String::from_utf8_lossy(&output)
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect())
}

fn has_conflict_markers(text: &str) -> bool {
    text.lines().any(|line| {
        line == "======="
            || ["<<<<<<<", ">>>>>>>"].iter().any(|marker| {
                line.strip_prefix(marker)
                    .is_some_and(|suffix| suffix.starts_with(' ') || suffix.starts_with('\t'))
            })
    })
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

/// Exact single-line Git output: remove only its final LF and reject non-UTF-8.
/// In particular, paths may contain leading/trailing whitespace and newlines.
pub(crate) fn git_output_line(
    folder: &Path,
    args: &[&str],
    action: &str,
) -> Result<String, String> {
    let mut bytes = git_output(folder, args, action)?;
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
    }
    String::from_utf8(bytes).map_err(|_| format!("Failed to {action}: Git output is not UTF-8"))
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

/// Read the remote and full upstream merge ref from branch tracking config.
fn upstream_config(path: &Path, branch: &str) -> Option<(String, String)> {
    Some((
        git_line(
            path,
            &["config", "--get", &format!("branch.{branch}.remote")],
        )?,
        git_line(
            path,
            &["config", "--get", &format!("branch.{branch}.merge")],
        )?,
    ))
}

/// Resolve the path to the repo's `.git` directory. Worktrees and
/// submodules can move it elsewhere; `git rev-parse --git-dir` is
/// the authoritative answer. Falls back to `<path>/.git` if the call
/// fails so paused-state detection still works on a basic repo.
fn resolve_git_dir(path: &Path) -> PathBuf {
    path.join(git_line(path, &["rev-parse", "--git-dir"]).unwrap_or_else(|| ".git".to_string()))
}

fn read_trimmed(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn ref_label(path: &Path, oid: &str) -> String {
    git_line(
        path,
        &[
            "for-each-ref",
            "--points-at",
            oid,
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
    )
    .and_then(|refs| refs.lines().next().map(str::to_owned))
    .unwrap_or_else(|| oid[..7.min(oid.len())].to_string())
}

fn commit_ref(path: &Path, oid: &str) -> Option<BranchCommitRef> {
    let output = git_output(
        path,
        &["log", "-1", "--format=%H%x00%h%x00%s%x00%an%x00%aI", oid],
        "read replayed commit",
    )
    .ok()?;
    let text = String::from_utf8_lossy(&output);
    let mut parts = text.trim_end().split('\0');
    Some(BranchCommitRef {
        hash: parts.next()?.to_string(),
        short_hash: parts.next()?.to_string(),
        subject: parts.next()?.to_string(),
        author: parts.next()?.to_string(),
        date: parts.next()?.to_string(),
    })
}

fn read_operation(path: &Path, git_dir: &Path) -> Option<GitOperation> {
    for (name, step_file, total_file) in [
        ("rebase-merge", "msgnum", "end"),
        ("rebase-apply", "next", "last"),
    ] {
        let dir = git_dir.join(name);
        if dir.is_dir() && !dir.join("applying").exists() {
            let head_name = read_trimmed(&dir.join("head-name"))
                .filter(|head| head != "detached HEAD")
                .map(|head| {
                    head.strip_prefix("refs/heads/")
                        .unwrap_or(&head)
                        .to_string()
                });
            return Some(GitOperation {
                kind: GitOperationKind::Rebase,
                target: read_trimmed(&dir.join("onto")).map(|oid| ref_label(path, &oid)),
                head_name,
                step: read_trimmed(&dir.join(step_file)).and_then(|step| step.parse().ok()),
                total: read_trimmed(&dir.join(total_file)).and_then(|total| total.parse().ok()),
                commit: read_trimmed(&git_dir.join("REBASE_HEAD"))
                    .and_then(|oid| commit_ref(path, &oid)),
            });
        }
    }
    for (sentinel, kind) in [
        ("MERGE_HEAD", GitOperationKind::Merge),
        ("CHERRY_PICK_HEAD", GitOperationKind::CherryPick),
        ("REVERT_HEAD", GitOperationKind::Revert),
    ] {
        let file = git_dir.join(sentinel);
        if file.exists() {
            let contents = read_trimmed(&file);
            let oid = contents.as_deref().and_then(|text| text.lines().next());
            return Some(GitOperation {
                kind,
                target: if kind == GitOperationKind::Merge {
                    oid.map(|oid| ref_label(path, oid))
                } else {
                    None
                },
                head_name: None,
                step: None,
                total: None,
                commit: if kind == GitOperationKind::Merge {
                    None
                } else {
                    oid.and_then(|oid| commit_ref(path, oid))
                },
            });
        }
    }
    let todo = read_trimmed(&git_dir.join("sequencer/todo"))?;
    let next = todo
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?;
    let kind = if next.starts_with("pick ") || next.starts_with("p ") {
        GitOperationKind::CherryPick
    } else if next.starts_with("revert ") {
        GitOperationKind::Revert
    } else {
        return None;
    };
    Some(GitOperation {
        kind,
        target: None,
        head_name: None,
        step: None,
        total: None,
        commit: None,
    })
}

fn read_message_template(git_dir: &Path) -> Option<String> {
    let text = read_trimmed(&git_dir.join("MERGE_MSG"))
        .or_else(|| read_trimmed(&git_dir.join("SQUASH_MSG")))?;
    let text = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Run a mutating git command, classifying known failures for callers.
///
/// Mutators intentionally omit `--no-optional-locks`. Read-only git
/// calls use it to avoid optional index locks during polling; writes
/// need Git's normal locking so index and ref updates serialize safely.
fn run_git_mutate(path: &Path, args: &[&str]) -> Result<(), ApiError> {
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
    let output = command
        .output()
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(mutation_error(&output))
    }
}

fn quoted(line: &str) -> Vec<&str> {
    line.split('\'').skip(1).step_by(2).collect()
}

/// Classify a failed mutation so callers act on known git failures instead of message text.
fn mutation_error(output: &std::process::Output) -> ApiError {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut message = [stdout.trim(), stderr.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if message.is_empty() {
        message = format!("git exited with {}", output.status);
    }
    if let Some(line) = stderr
        .lines()
        .find(|line| line.contains("used by worktree at '"))
    {
        let parts = quoted(line);
        if let [branch, path, ..] = parts.as_slice() {
            return ApiError::BranchInWorktree {
                branch: (*branch).to_string(),
                path: (*path).to_string(),
            };
        }
    }
    if let Some(line) = stderr
        .lines()
        .find(|line| line.contains("contains modified or untracked files"))
    {
        if let Some(path) = quoted(line).first() {
            return ApiError::WorktreeHasChanges {
                path: (*path).to_string(),
                message,
            };
        }
    }
    if let Some(line) = stderr
        .lines()
        .find(|line| line.contains("not fully merged"))
    {
        if let Some(branch) = quoted(line).first() {
            return ApiError::BranchUnmerged {
                branch: (*branch).to_string(),
                message,
            };
        }
    }
    if stderr.contains("failed to push some refs") {
        return ApiError::PushRejected { message };
    }
    if [
        "CONFLICT (",
        "unmerged files",
        "needs merge",
        "edit all merge conflicts",
    ]
    .iter()
    .any(|marker| stdout.contains(marker) || stderr.contains(marker))
    {
        return ApiError::MergeConflicts { message };
    }
    ApiError::Internal(message)
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

    /// A summary computed before an invalidation must never re-populate the
    /// cache after it, otherwise a slow pre-mutation `git status` resurrects
    /// stale state for a full TTL (the staged → unstaged → staged flicker).
    #[test]
    fn stale_summary_never_repopulates_cache() {
        let svc = GitService::new();
        let path = Path::new("/nonexistent/sworm-cache-test");
        let summary = GitSummary::default();

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
    fn undo_root_commit_leaves_unborn_branch_with_staged_files() {
        let repo = temp_repo("undo-root");
        let service = GitService::new();
        assert!(!service.get_summary(&repo).unwrap().unborn);

        assert_eq!(service.undo_last_commit(&repo).unwrap(), "init");
        let head = Command::new("git")
            .args(["rev-parse", "--verify", "--quiet", "HEAD"])
            .current_dir(&repo)
            .output()
            .unwrap();
        assert!(!head.status.success());
        let summary = service.get_summary(&repo).unwrap();
        assert!(summary.unborn);
        assert_eq!(summary.branch.as_deref(), Some("main"));
        assert!(summary
            .changes
            .iter()
            .any(|change| change.path == "a.txt" && change.staged));
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn undo_in_shallow_clone_keeps_branch() {
        let source = temp_repo("undo-shallow-src");
        std::fs::write(source.join("a.txt"), "two\n").unwrap();
        git(&source, &["commit", "-qam", "two"]);
        let clone =
            std::env::temp_dir().join(format!("sworm-undo-shallow-{}", uuid::Uuid::new_v4()));
        git(
            &source,
            &[
                "clone",
                "-q",
                "--depth",
                "1",
                &format!("file://{}", source.display()),
                clone.to_str().unwrap(),
            ],
        );
        let before = git_line(&clone, &["rev-parse", "refs/heads/main"]);
        assert!(GitService::new().undo_last_commit(&clone).is_err());
        assert_eq!(git_line(&clone, &["rev-parse", "refs/heads/main"]), before);
        std::fs::remove_dir_all(clone).unwrap();
        std::fs::remove_dir_all(source).unwrap();
    }

    #[test]
    fn undo_rejects_paused_operation() {
        let repo = temp_repo("undo-paused");
        std::fs::write(repo.join("a.txt"), "two\n").unwrap();
        git(&repo, &["commit", "-qam", "two"]);
        let svc = GitService::new();
        let oid = git_line(&repo, &["rev-parse", "HEAD"]).unwrap();
        std::fs::write(repo.join(".git/MERGE_HEAD"), &oid).unwrap();
        assert!(matches!(
            svc.undo_last_commit(&repo),
            Err(ApiError::InvalidArgument(_))
        ));
        assert_eq!(
            git_line(&repo, &["rev-parse", "HEAD"]).as_deref(),
            Some(oid.as_str())
        );
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn list_remotes_reports_urls_and_push_default() {
        let repo = temp_repo("list-remotes");
        git(&repo, &["remote", "add", "origin", "/tmp/a.git"]);
        git(&repo, &["remote", "add", "upstream", "/tmp/b.git"]);
        git(
            &repo,
            &["remote", "set-url", "--push", "origin", "/tmp/c.git"],
        );
        git(&repo, &["config", "remote.pushDefault", "upstream"]);

        let remotes = GitService::new().list_remotes(&repo).unwrap();
        assert_eq!(remotes.len(), 2);
        assert_eq!(remotes[0].name, "origin");
        assert_eq!(remotes[0].fetch_url.as_deref(), Some("/tmp/a.git"));
        assert_eq!(remotes[0].push_url.as_deref(), Some("/tmp/c.git"));
        assert!(!remotes[0].push_default);
        assert_eq!(remotes[1].name, "upstream");
        assert_eq!(remotes[1].fetch_url.as_deref(), Some("/tmp/b.git"));
        assert_eq!(remotes[1].push_url.as_deref(), Some("/tmp/b.git"));
        assert!(remotes[1].push_default);
        std::fs::remove_dir_all(repo).unwrap();
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

        GitService::new()
            .sequencer(
                &repo,
                GitOperationKind::Merge,
                &GitSequencerAction::Continue { message: None },
            )
            .unwrap();
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
            .discard(&repo, Some(&["a.txt".into(), "new".into()]), true)
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
            .discard(&repo, Some(&["a.txt".into()]), true)
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
            svc.discard(&repo, None, true),
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(matches!(
            svc.discard(&repo, Some(&["a.txt".into(), "u.txt".into()]), true),
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(repo.join("u.txt").exists());

        svc.discard(&repo, Some(&["u.txt".into()]), true).unwrap();
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
            sender
                .send(GitService::new().push(&push_repo, None, false))
                .ok();
        });
        let result = receiver.recv_timeout(Duration::from_secs(20));
        stop.store(true, Ordering::Relaxed);
        server.join().unwrap();
        std::fs::remove_dir_all(repo).unwrap();
        let error = result
            .expect("push must finish without credential prompts")
            .unwrap_err();
        push.join().unwrap();
        assert!(
            error.to_string().contains("terminal prompts disabled"),
            "{error}"
        );
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

        GitService::new()
            .commit(&repo, "x", &GitCommitOptions::default())
            .unwrap();
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
            ApiError::BranchUnmerged { branch, message } => {
                assert_eq!(branch, "topic");
                assert!(message.to_lowercase().contains("not fully merged"));
            }
            error => panic!("expected BranchUnmerged, got: {error}"),
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_operation_detects_sentinels() {
        let repo = temp_repo("sentinels");
        let git_dir = repo.join(".git");
        let oid = git_line(&repo, &["rev-parse", "HEAD"]).unwrap();
        assert!(read_operation(&repo, &git_dir).is_none());
        std::fs::create_dir(git_dir.join("rebase-merge")).unwrap();
        let operation = read_operation(&repo, &git_dir).unwrap();
        assert_eq!(operation.kind, GitOperationKind::Rebase);
        assert!(operation.target.is_none() && operation.head_name.is_none());
        assert!(
            operation.step.is_none() && operation.total.is_none() && operation.commit.is_none()
        );
        std::fs::remove_dir(git_dir.join("rebase-merge")).unwrap();
        std::fs::create_dir(git_dir.join("rebase-apply")).unwrap();
        std::fs::write(git_dir.join("rebase-apply/applying"), "").unwrap();
        assert!(read_operation(&repo, &git_dir).is_none());
        std::fs::remove_dir_all(git_dir.join("rebase-apply")).unwrap();
        std::fs::write(git_dir.join("MERGE_HEAD"), &oid).unwrap();
        let operation = read_operation(&repo, &git_dir).unwrap();
        assert_eq!(operation.kind, GitOperationKind::Merge);
        assert_eq!(operation.target.as_deref(), Some("main"));
        std::fs::remove_file(git_dir.join("MERGE_HEAD")).unwrap();
        std::fs::write(git_dir.join("CHERRY_PICK_HEAD"), &oid).unwrap();
        let operation = read_operation(&repo, &git_dir).unwrap();
        assert_eq!(operation.kind, GitOperationKind::CherryPick);
        assert_eq!(operation.commit.unwrap().subject, "init");
        std::fs::remove_file(git_dir.join("CHERRY_PICK_HEAD")).unwrap();
        std::fs::write(git_dir.join("REVERT_HEAD"), &oid).unwrap();
        assert_eq!(
            read_operation(&repo, &git_dir).unwrap().kind,
            GitOperationKind::Revert
        );
        std::fs::remove_file(git_dir.join("REVERT_HEAD")).unwrap();
        std::fs::create_dir(git_dir.join("sequencer")).unwrap();
        std::fs::write(git_dir.join("sequencer/todo"), "revert abc x\n").unwrap();
        assert_eq!(
            read_operation(&repo, &git_dir).unwrap().kind,
            GitOperationKind::Revert
        );
        std::fs::remove_dir_all(repo).unwrap();
    }

    /// Diverged branches editing the same line, ready for merge or rebase.
    fn conflicting_branches(tag: &str) -> PathBuf {
        let repo = temp_repo(tag);
        git(&repo, &["switch", "-q", "-c", "feature"]);
        std::fs::write(repo.join("a.txt"), "feature\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "feature"]);
        git(&repo, &["switch", "-q", "main"]);
        std::fs::write(repo.join("a.txt"), "main\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "main"]);
        repo
    }

    fn bare_remote(repo: &Path) -> PathBuf {
        let remote = repo.with_extension("remote.git");
        git(repo, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        remote
    }

    #[test]
    fn mutation_errors_are_typed() {
        let service = GitService::new();
        let repo = temp_repo("typed-errors");
        assert!(matches!(
            service.push(&repo, None, false),
            Err(ApiError::NoUpstream { branch }) if branch == "main"
        ));
        let remote = bare_remote(&repo);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        let clone = repo.with_extension("clone");
        git(
            &repo,
            &[
                "clone",
                "-q",
                "--branch",
                "main",
                remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        git(&clone, &["config", "user.email", "t@t.com"]);
        git(&clone, &["config", "user.name", "t"]);
        std::fs::write(clone.join("remote.txt"), "remote\n").unwrap();
        git(&clone, &["add", "remote.txt"]);
        git(&clone, &["commit", "-q", "-m", "remote"]);
        git(&clone, &["push", "-q"]);
        std::fs::write(repo.join("local.txt"), "local\n").unwrap();
        git(&repo, &["add", "local.txt"]);
        git(&repo, &["commit", "-q", "-m", "local"]);
        assert!(matches!(
            service.push(&repo, None, false),
            Err(ApiError::PushRejected { .. })
        ));

        let worktree = repo.with_extension("worktree");
        git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "wtb",
                worktree.to_str().unwrap(),
            ],
        );
        assert!(matches!(
            service.checkout_branch(&repo, "wtb"),
            Err(ApiError::BranchInWorktree { branch, path })
                if branch == "wtb" && Path::new(&path) == worktree
        ));
        std::fs::write(worktree.join("dirty.txt"), "dirty\n").unwrap();
        assert!(matches!(
            run_git_mutate(&repo, &["worktree", "remove", worktree.to_str().unwrap()]),
            Err(ApiError::WorktreeHasChanges { path, .. }) if Path::new(&path) == worktree
        ));
        git(
            &repo,
            &["worktree", "remove", "--force", worktree.to_str().unwrap()],
        );
        std::fs::remove_dir_all(repo).unwrap();
        std::fs::remove_dir_all(remote).unwrap();
        std::fs::remove_dir_all(clone).unwrap();

        let repo = conflicting_branches("typed-conflicts");
        assert!(matches!(
            service.merge_into_current(&repo, "feature", false),
            Err(ApiError::MergeConflicts { message }) if message.contains("CONFLICT")
        ));
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn stage_refuses_conflict_markers_unless_allowed() {
        let repo = conflicting_branches("stage-markers");
        let service = GitService::new();
        assert!(service.merge_into_current(&repo, "feature", false).is_err());
        let files = ["a.txt".to_string()];
        assert!(matches!(
            service.stage(&repo, Some(&files), false),
            Err(ApiError::ConflictMarkers { paths }) if paths == files
        ));
        service.stage(&repo, Some(&files), true).unwrap();
        assert!(unmerged_paths(&repo, &[]).unwrap().is_empty());
        git(&repo, &["merge", "--abort"]);
        assert!(service.merge_into_current(&repo, "feature", false).is_err());
        std::fs::write(repo.join("a.txt"), "resolved\n").unwrap();
        service.stage(&repo, Some(&files), false).unwrap();
        assert!(unmerged_paths(&repo, &[]).unwrap().is_empty());
        assert!(has_conflict_markers("<<<<<<< ours\r\n"));
        assert!(has_conflict_markers("=======\r\n"));
        assert!(has_conflict_markers(">>>>>>>\ttheirs\n"));
        assert!(!has_conflict_markers("<<<<<<<not a marker\n========\n"));
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn take_side_resolves_with_chosen_version() {
        let repo = conflicting_branches("take-side");
        let service = GitService::new();
        let files = ["a.txt".to_string()];
        assert!(service.merge_into_current(&repo, "feature", false).is_err());
        service
            .take_conflict_side(&repo, &files, GitConflictSide::Theirs)
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join("a.txt")).unwrap(),
            "feature\n"
        );
        assert!(unmerged_paths(&repo, &[]).unwrap().is_empty());
        git(&repo, &["merge", "--abort"]);
        git(&repo, &["switch", "-q", "feature"]);
        git(&repo, &["rm", "a.txt"]);
        git(&repo, &["commit", "-q", "-m", "delete"]);
        git(&repo, &["switch", "-q", "main"]);
        assert!(service.merge_into_current(&repo, "feature", false).is_err());
        let conflicts = service.working_status(&repo).unwrap().conflicts;
        assert_eq!(conflicts[0].kind, GitConflictKind::DeletedByThem);
        service
            .take_conflict_side(&repo, &files, GitConflictSide::Theirs)
            .unwrap();
        assert!(!repo.join("a.txt").exists());
        assert!(unmerged_paths(&repo, &[]).unwrap().is_empty());
        assert!(
            git_output(&repo, &["ls-files", "--", "a.txt"], "read index")
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn publish_then_sync_pushes_ahead_commits() {
        let repo = temp_repo("publish-sync");
        let remote = bare_remote(&repo);
        let service = GitService::new();
        service.publish(&repo, None, None).unwrap();
        assert_eq!(
            git_line(&repo, &["config", "--get", "branch.main.merge"]).as_deref(),
            Some("refs/heads/main")
        );
        std::fs::write(repo.join("a.txt"), "ahead\n").unwrap();
        git(&repo, &["commit", "-q", "-am", "ahead"]);
        git(&repo, &["config", "pull.rebase", "false"]);
        service.sync(&repo, false).unwrap();
        assert_eq!(
            git_line(&remote, &["rev-parse", "refs/heads/main"]),
            git_line(&repo, &["rev-parse", "HEAD"]),
        );
        std::fs::remove_dir_all(repo).unwrap();
        std::fs::remove_dir_all(remote).unwrap();
    }

    #[test]
    fn ignore_appends_entries_on_own_lines() {
        let repo = temp_repo("ignore");
        let service = GitService::new();
        service.ignore(&repo, &[]).unwrap();
        assert!(!repo.join(".gitignore").exists());
        std::fs::write(repo.join(".gitignore"), "x").unwrap();
        service.ignore(&repo, &[]).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).unwrap(),
            "x"
        );
        assert!(!service.summary_cache.lock().contains_key(&repo));
        service.ignore(&repo, &["a[1].txt".to_string()]).unwrap();
        assert_eq!(
            std::fs::read_to_string(repo.join(".gitignore")).unwrap(),
            "x\na\\[1].txt\n"
        );
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn sequencer_continue_with_unresolved_rebase_returns_merge_conflicts() {
        let repo = conflicting_branches("rebase-unresolved");
        let service = GitService::new();
        git(&repo, &["switch", "-q", "feature"]);
        assert!(matches!(
            service.rebase_current_onto(&repo, "main"),
            Err(ApiError::MergeConflicts { .. })
        ));
        assert!(matches!(
            service.sequencer(
                &repo,
                GitOperationKind::Rebase,
                &GitSequencerAction::Continue { message: None },
            ),
            Err(ApiError::MergeConflicts { .. })
        ));
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn sequencer_continue_with_message_commits_and_finishes_rebase() {
        let repo = conflicting_branches("rebase-message");
        let service = GitService::new();
        git(&repo, &["switch", "-q", "feature"]);
        assert!(service.rebase_current_onto(&repo, "main").is_err());
        std::fs::write(repo.join("a.txt"), "resolved\n").unwrap();
        git(&repo, &["add", "a.txt"]);
        service
            .sequencer(
                &repo,
                GitOperationKind::Rebase,
                &GitSequencerAction::Continue {
                    message: Some("custom".to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            git_line(&repo, &["log", "-1", "--format=%s"]).as_deref(),
            Some("custom")
        );
        assert!(read_operation(&repo, &resolve_git_dir(&repo)).is_none());
        std::fs::remove_dir_all(repo).unwrap();
    }

    #[test]
    fn summary_reports_paused_rebase() {
        let repo = conflicting_branches("rebase-summary");
        let remote = bare_remote(&repo);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        git(&repo, &["switch", "-q", "feature"]);
        git(
            &repo,
            &["branch", "--set-upstream-to=origin/main", "feature"],
        );
        let service = GitService::new();
        assert_eq!(
            service.get_summary(&repo).unwrap().upstream.as_deref(),
            Some("origin/main")
        );
        assert!(service.rebase_current_onto(&repo, "main").is_err());
        service.invalidate(&repo);
        let summary = service.get_summary(&repo).unwrap();
        assert!(summary.detached);
        let operation = summary.operation.unwrap();
        assert_eq!(operation.kind, GitOperationKind::Rebase);
        assert_eq!(operation.head_name.as_deref(), Some("feature"));
        assert_eq!(operation.target.as_deref(), Some("main"));
        assert_eq!((operation.step, operation.total), (Some(1), Some(1)));
        assert_eq!(operation.commit.unwrap().subject, "feature");
        assert_eq!(summary.conflicts.len(), 1);
        assert_eq!(summary.conflicts[0].path, "a.txt");
        assert_eq!(summary.conflicts[0].kind, GitConflictKind::BothModified);
        assert!(summary.message_template.unwrap().starts_with("feature"));
        service
            .sequencer(&repo, GitOperationKind::Rebase, &GitSequencerAction::Abort)
            .unwrap();
        assert!(service.get_summary(&repo).unwrap().operation.is_none());
        std::fs::remove_dir_all(repo).unwrap();
        std::fs::remove_dir_all(remote).unwrap();
    }
}
