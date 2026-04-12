use serde::Serialize;
use std::path::Path;

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

#[derive(Debug, Clone, Serialize)]
pub struct GitCommit {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// Structured diff data including file content for diff viewer rendering.
#[derive(Debug, Clone, Serialize)]
pub struct DiffContext {
    pub raw_diff: String,
    pub old_content: Option<String>,
    pub new_content: Option<String>,
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
        let raw_diff = self.get_file_diff(path, file_path, staged);
        let old_content = self.get_old_content(path, file_path);
        let new_content = self.get_new_content(path, file_path, staged);

        // For untracked files, git diff returns nothing but we still have content.
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

    pub fn get_log(&self, path: &Path, limit: usize) -> Vec<GitCommit> {
        let output = std::process::Command::new("git")
            .args([
                "--no-optional-locks",
                "log",
                &format!("--max-count={}", limit),
                "--format=%H%n%h%n%an%n%aI%n%s",
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
            .chunks(5)
            .filter_map(|chunk| {
                if chunk.len() != 5 {
                    return None;
                }

                Some(GitCommit {
                    hash: chunk[0].clone(),
                    short_hash: chunk[1].clone(),
                    author: chunk[2].clone(),
                    date: chunk[3].clone(),
                    message: chunk[4].clone(),
                })
            })
            .collect()
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
