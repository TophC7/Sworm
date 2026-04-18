use serde::Serialize;
use std::path::Path;
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

/// Structured diff data including file content for diff viewer rendering.
#[derive(Debug, Clone, Serialize)]
pub struct DiffContext {
    pub raw_diff: String,
    pub old_content: Option<String>,
    pub new_content: Option<String>,
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
const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024; // 2 MiB

/// Git service using the system git CLI.
pub struct GitService;

impl GitService {
    pub fn new() -> Self {
        Self
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
    pub fn get_summary(&self, path: &Path) -> GitSummary {
        if !self.is_git_repo(path) {
            return GitSummary {
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
        }

        let branch = self.current_branch(path);
        let base_ref = self.default_base_ref(path);
        let (ahead, behind) = self.ahead_behind(path, &branch, &base_ref);
        let changes = self.get_changes(path);

        let staged_count = changes.iter().filter(|c| c.staged).count() as i32;
        let unstaged_count = changes
            .iter()
            .filter(|c| !c.staged && c.status != "?")
            .count() as i32;
        let untracked_count = changes.iter().filter(|c| c.status == "?").count() as i32;

        GitSummary {
            is_repo: true,
            branch,
            base_ref,
            ahead,
            behind,
            changes,
            staged_count,
            unstaged_count,
            untracked_count,
        }
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

        // Get numstat for unstaged diff stats
        merge_numstat(&mut changes, path, &["diff", "--numstat"], false);

        // Get numstat for staged diff stats
        merge_numstat(&mut changes, path, &["diff", "--cached", "--numstat"], true);

        changes
    }

    /// Get ahead/behind counts relative to the tracking branch.
    fn ahead_behind(
        &self,
        path: &Path,
        _branch: &Option<String>,
        _base: &Option<String>,
    ) -> (Option<i32>, Option<i32>) {
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
        // Run staged and unstaged diffs in parallel — both are independent reads.
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

    /// Get diff for a specific file.
    pub fn get_file_diff(&self, path: &Path, file_path: &str, staged: bool) -> Option<String> {
        let mut args = vec!["diff"];
        if staged {
            args.push("--cached");
        }
        args.push("--");
        args.push(file_path);

        let output = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            None
        }
    }

    /// Get structured diff context with file content for the diff viewer.
    pub fn get_diff_context(
        &self,
        path: &Path,
        file_path: &str,
        staged: bool,
    ) -> Option<DiffContext> {
        let raw_diff = self
            .get_file_diff(path, file_path, staged)
            .filter(|d| !d.trim().is_empty());
        let old_content = self.get_old_content(path, file_path);
        let new_content = self.get_new_content(path, file_path, staged);

        // For untracked files, git diff returns empty but we still have content.
        // `git diff --no-index` exits with code 1 when differences exist (always
        // true for new-file vs /dev/null), so we check stdout length rather than
        // exit status — this is intentional, not an oversight.
        if raw_diff.is_none() && new_content.is_some() && old_content.is_none() {
            let output = std::process::Command::new("git")
                .args(["diff", "--no-index", "--", "/dev/null", file_path])
                .current_dir(path)
                .output()
                .ok();

            let synthetic_diff = output
                .filter(|o| !o.stdout.is_empty())
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string());

            return Some(DiffContext {
                raw_diff: synthetic_diff.unwrap_or_default(),
                old_content: None,
                new_content,
            });
        }

        raw_diff.map(|diff| DiffContext {
            raw_diff: diff,
            old_content,
            new_content,
        })
    }

    /// Get file content at HEAD, returning `None` for binary or oversized files.
    fn get_old_content(&self, path: &Path, file_path: &str) -> Option<String> {
        let output = std::process::Command::new("git")
            .args([
                "--no-optional-locks",
                "show",
                &format!("HEAD:{}", file_path),
            ])
            .current_dir(path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        guard_content(output.stdout)
    }

    /// Get new file content (working tree or staged index).
    /// Returns `None` for binary or oversized files.
    fn get_new_content(&self, path: &Path, file_path: &str, staged: bool) -> Option<String> {
        if staged {
            let output = std::process::Command::new("git")
                .args(["--no-optional-locks", "show", &format!(":{}", file_path)])
                .current_dir(path)
                .output()
                .ok()?;

            if !output.status.success() {
                return None;
            }

            guard_content(output.stdout)
        } else {
            let full_path = path.join(file_path);
            let meta = std::fs::metadata(&full_path).ok()?;
            if meta.len() > MAX_CONTENT_BYTES as u64 {
                return None;
            }
            let bytes = std::fs::read(&full_path).ok()?;
            guard_content(bytes)
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

        // 2. File stats — diff against first parent (like GitHub).
        //    For root commits, diff-tree --root shows everything as added.
        let diff_ref = if parents.is_empty() {
            // Root commit — use diff-tree --root
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

    /// Get all file diffs for a commit in one git call.
    pub fn get_commit_diffs(
        &self,
        path: &Path,
        hash: &str,
    ) -> std::collections::HashMap<String, String> {
        Self::split_diff_patch(&self.full_commit_patch(path, hash))
    }

    /// Get all working-tree diffs in one git call (staged or unstaged).
    /// Includes synthetic diffs for untracked files.
    pub fn get_working_diffs(
        &self,
        path: &Path,
        staged: bool,
        untracked_paths: &[String],
    ) -> std::collections::HashMap<String, String> {
        let mut args = vec!["diff"];
        if staged {
            args.push("--cached");
        }

        let output = std::process::Command::new("git")
            .args(&args)
            .current_dir(path)
            .output();

        let full = match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        };

        let mut result = Self::split_diff_patch(&full);

        // Synthetic diffs for untracked files (not in git diff output)
        for file_path in untracked_paths {
            let output = std::process::Command::new("git")
                .args(["diff", "--no-index", "--", "/dev/null", file_path])
                .current_dir(path)
                .output();

            if let Some(diff) = output
                .ok()
                .filter(|o| !o.stdout.is_empty() && o.stdout.len() <= MAX_CONTENT_BYTES)
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            {
                result.insert(file_path.clone(), diff);
            }
        }

        result
    }

    fn split_diff_patch(full: &str) -> std::collections::HashMap<String, String> {
        let mut result = std::collections::HashMap::new();
        if full.is_empty() {
            return result;
        }

        let mut start = if full.starts_with("diff --git ") {
            0
        } else {
            match full.find("diff --git ") {
                Some(i) => i,
                None => return result,
            }
        };

        loop {
            let next = full[start + 1..].find("diff --git ").map(|i| i + start + 1);
            let end = next.unwrap_or(full.len());
            let chunk = &full[start..end];

            if let Some(file_path) = Self::extract_diff_path(chunk) {
                if chunk.len() <= MAX_CONTENT_BYTES {
                    result.insert(file_path, chunk.to_string());
                }
            }

            match next {
                Some(n) => start = n,
                None => break,
            }
        }

        result
    }

    fn full_commit_patch(&self, path: &Path, hash: &str) -> String {
        // Try first-parent diff
        let output = std::process::Command::new("git")
            .args(["diff", &format!("{}^..{}", hash, hash)])
            .current_dir(path)
            .output();

        if let Ok(ref o) = output {
            if o.status.success() {
                let s = String::from_utf8_lossy(&o.stdout);
                if !s.trim().is_empty() {
                    return s.to_string();
                }
            }
        }

        // Fallback for root commits
        let output = std::process::Command::new("git")
            .args(["diff-tree", "--root", "-p", "--no-commit-id", "-r", hash])
            .current_dir(path)
            .output();

        match output {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => String::new(),
        }
    }

    fn extract_diff_path(chunk: &str) -> Option<String> {
        // "diff --git a/... b/PATH\n..."
        let first_line = chunk.lines().next()?;
        let b_pos = first_line.rfind(" b/")?;
        let raw = &first_line[b_pos + 3..];
        Some(raw.trim_matches('"').to_string())
    }

    // ── Write operations ────────────────────────────────────────────

    /// Stage all changes (tracked + untracked).
    pub fn stage_all(&self, path: &Path) -> Result<(), String> {
        run_git_mutate(path, &["add", "-A"])
    }

    /// Stage specific files or directories.
    pub fn stage_files(&self, path: &Path, files: &[String]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["add", "--"];
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(refs);
        run_git_mutate(path, &args)
    }

    /// Unstage all staged changes back to the working tree.
    pub fn unstage_all(&self, path: &Path) -> Result<(), String> {
        run_git_mutate(path, &["reset", "HEAD"])
    }

    /// Unstage specific files or directories.
    pub fn unstage_files(&self, path: &Path, files: &[String]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }
        let mut args = vec!["reset", "HEAD", "--"];
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
        args.extend(refs);
        run_git_mutate(path, &args)
    }

    /// Discard all unstaged changes and remove untracked files.
    pub fn discard_all(&self, path: &Path) -> Result<(), String> {
        // Restore tracked files to HEAD state
        run_git_mutate(path, &["checkout", "--", "."])?;
        // Remove untracked files and directories (but not ignored ones)
        run_git_mutate(path, &["clean", "-fd"])
    }

    /// Discard changes for specific files or directories.
    /// Handles both tracked (checkout) and untracked (clean) files in one
    /// invocation each. Errors are ignored because a mixed set of paths
    /// will always fail one of the two commands — e.g. checkout rejects
    /// untracked paths, clean is a no-op for tracked ones.
    pub fn discard_files(&self, path: &Path, files: &[String]) -> Result<(), String> {
        if files.is_empty() {
            return Ok(());
        }
        let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();

        let mut checkout_args = vec!["checkout", "--"];
        checkout_args.extend(refs.iter().copied());
        let _ = run_git_mutate(path, &checkout_args);

        let mut clean_args = vec!["clean", "-fd", "--"];
        clean_args.extend(refs.iter().copied());
        let _ = run_git_mutate(path, &clean_args);

        Ok(())
    }

    /// Create a commit with the given message. Returns the new short hash.
    pub fn commit(&self, path: &Path, message: &str) -> Result<String, String> {
        run_git_mutate(path, &["commit", "-m", message])?;
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

        run_git_mutate(path, &["reset", "--soft", "HEAD~1"])?;
        Ok(message)
    }

    /// Push current branch to its upstream remote.
    pub fn push(&self, path: &Path) -> Result<(), String> {
        run_git_mutate(path, &["push"])
    }

    /// Push with --force-with-lease (safe force push).
    pub fn push_force_with_lease(&self, path: &Path) -> Result<(), String> {
        run_git_mutate(path, &["push", "--force-with-lease"])
    }

    /// Pull from the upstream remote (fetch + merge).
    pub fn pull(&self, path: &Path) -> Result<(), String> {
        run_git_mutate(path, &["pull"])
    }

    /// Fetch from all remotes.
    pub fn fetch(&self, path: &Path) -> Result<(), String> {
        run_git_mutate(path, &["fetch", "--all"])
    }

    /// Stash all changes including untracked files.
    pub fn stash_all(&self, path: &Path, message: Option<&str>) -> Result<(), String> {
        let mut args = vec!["stash", "push", "--include-untracked"];
        if let Some(msg) = message {
            args.push("-m");
            args.push(msg);
        }
        run_git_mutate(path, &args)
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
        run_git_mutate(path, &["stash", "pop", &stash_ref])
    }

    /// Drop a stash entry by index without applying it.
    pub fn stash_drop(&self, path: &Path, index: usize) -> Result<(), String> {
        let stash_ref = format!("stash@{{{}}}", index);
        run_git_mutate(path, &["stash", "drop", &stash_ref])
    }

    /// Get all file diffs for a stash entry (including untracked files).
    pub fn get_stash_diffs(
        &self,
        path: &Path,
        index: usize,
    ) -> std::collections::HashMap<String, String> {
        let stash_ref = format!("stash@{{{}}}", index);

        // Try with --include-untracked first (git 2.32+), fall back without
        let output = std::process::Command::new("git")
            .args(["stash", "show", "-p", "-u", &stash_ref])
            .current_dir(path)
            .output();

        let full = match output {
            Ok(ref o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => {
                // Fallback without untracked
                let output = std::process::Command::new("git")
                    .args(["stash", "show", "-p", &stash_ref])
                    .current_dir(path)
                    .output();
                match output {
                    Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).to_string(),
                    _ => return std::collections::HashMap::new(),
                }
            }
        };

        Self::split_diff_patch(&full)
    }

    /// Initialize a new git repository.
    pub fn init(&self, path: &Path) -> Result<(), String> {
        let output = std::process::Command::new("git")
            .args(["init"])
            .current_dir(path)
            .output()
            .map_err(|e| format!("git init failed: {}", e))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
    }

    /// Clone a repository into the given directory (in-place, no subfolder).
    ///
    /// Uses `git clone <url> .` for empty dirs, or init + remote + fetch + checkout
    /// for dirs with existing content.
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
            return self.run_git_cmd(path, &["clone", url, "."]);
        }

        self.run_git_cmd(path, &["init"])?;
        self.run_git_cmd(path, &["remote", "add", "origin", url])?;
        self.run_git_cmd(path, &["fetch", "origin"])?;

        let default_branch = self.detect_remote_default_branch(path);
        self.run_git_cmd(
            path,
            &[
                "checkout",
                "-b",
                &default_branch,
                &format!("origin/{}", default_branch),
            ],
        )
    }

    fn run_git_cmd(&self, path: &Path, args: &[&str]) -> Result<(), String> {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .map_err(|e| format!("git {} failed: {}", args[0], e))?;

        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
        Ok(())
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

/// Return content as a String if it's within the size limit and looks
/// like text (no null bytes in the first 8 KiB). Returns `None` for
/// binary or oversized content so the frontend falls back gracefully.
fn guard_content(bytes: Vec<u8>) -> Option<String> {
    if bytes.len() > MAX_CONTENT_BYTES {
        return None;
    }
    let probe_len = bytes.len().min(8192);
    if bytes[..probe_len].contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
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
