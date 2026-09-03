//! Keeps the git sidebar honest about repository state.
//!
//! Watches the folder's git dir non-recursively (HEAD, index, ORIG_HEAD,
//! FETCH_HEAD, MERGE_HEAD, packed-refs, rebase/merge markers) plus the
//! common dir's `refs/` tree recursively (branches, remotes, tags, stash).
//! `objects/` and `logs/` are never watched: every commit writes there and
//! the ref update already tells us what we need. This is the same split
//! VS Code's `DotGitWatcher` uses.
//!
//! Events are coalesced for 300ms and emitted as `git-changed` with the
//! touched git-dir-relative paths, so the frontend can tell an index-only
//! change (refresh status) from a ref change (refresh graph, branches,
//! stashes too). The summary cache is invalidated before emitting so the
//! refresh this triggers cannot be served a pre-change summary.

use crate::services::git::GitService;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;

pub const GIT_CHANGED_EVENT: &str = "git-changed";

const FLUSH_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize)]
pub struct GitChangedEvent {
    pub folder_path: String,
    /// Paths relative to the git dir (`index`, `HEAD`, `refs/heads/main`, …).
    pub paths: Vec<String>,
}

pub struct GitWatcherService {
    folders: Mutex<HashMap<PathBuf, RecommendedWatcher>>,
    git: Arc<GitService>,
}

impl GitWatcherService {
    pub fn new(git: Arc<GitService>) -> Self {
        Self {
            folders: Mutex::new(HashMap::new()),
            git,
        }
    }

    /// Start watching `folder`'s git dir; a no-op when already watched.
    /// Errors when the folder is not inside a git repository or the watcher
    /// cannot be created — the caller logs and moves on, since the sidebar
    /// still refreshes on focus and after in-app actions.
    pub fn watch(&self, app: &tauri::AppHandle, folder: &Path) -> Result<(), String> {
        let mut folders = self.folders.lock();
        if folders.contains_key(folder) {
            return Ok(());
        }
        let (git_dir, common_dir) = resolve_git_dirs(folder)?;
        tracing::debug!(folder = %folder.display(), git_dir = %git_dir.display(), "git watcher started");
        let watcher = spawn_watch(app, Arc::clone(&self.git), folder, git_dir, common_dir)?;
        folders.insert(folder.to_path_buf(), watcher);
        Ok(())
    }

    pub fn stop(&self, folder: &Path) {
        self.folders.lock().remove(folder);
    }
}

/// `(git_dir, common_dir)`; they differ inside a linked worktree, where refs
/// live in the common dir and HEAD/index in the worktree's own git dir.
fn resolve_git_dirs(folder: &Path) -> Result<(PathBuf, PathBuf), String> {
    let output = std::process::Command::new("git")
        .args([
            "--no-optional-locks",
            "rev-parse",
            "--git-dir",
            "--git-common-dir",
        ])
        .current_dir(folder)
        .output()
        .map_err(|error| format!("Failed to run git: {error}"))?;
    if !output.status.success() {
        return Err("not a git repository".to_string());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let git_dir = lines
        .next()
        .map(|line| absolute(folder, line))
        .ok_or_else(|| "not a git repository".to_string())?;
    let common_dir = lines
        .next()
        .map(|line| absolute(folder, line))
        .unwrap_or_else(|| git_dir.clone());
    Ok((git_dir, common_dir))
}

fn absolute(folder: &Path, rel: &str) -> PathBuf {
    let path = PathBuf::from(rel);
    if path.is_absolute() {
        path
    } else {
        folder.join(path)
    }
}

fn spawn_watch(
    app: &tauri::AppHandle,
    git: Arc<GitService>,
    folder: &Path,
    git_dir: PathBuf,
    common_dir: PathBuf,
) -> Result<RecommendedWatcher, String> {
    let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
    let flush_scheduled = Arc::new(AtomicBool::new(false));

    let handle = app.clone();
    let folder_for_events = folder.to_path_buf();
    let git_dir_for_events = git_dir.clone();
    let common_dir_for_events = common_dir.clone();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        let touched: Vec<String> = event
            .paths
            .iter()
            .filter_map(|path| relative_git_path(&git_dir_for_events, &common_dir_for_events, path))
            .filter(|rel| !is_noise(rel))
            .collect();
        if touched.is_empty() {
            return;
        }

        pending.lock().extend(touched);
        if flush_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }

        let handle = handle.clone();
        let git = Arc::clone(&git);
        let folder = folder_for_events.clone();
        let pending = Arc::clone(&pending);
        let flush_scheduled = Arc::clone(&flush_scheduled);
        std::thread::spawn(move || {
            std::thread::sleep(FLUSH_DELAY);
            // Re-arm under the `pending` lock: a producer that already added to
            // `pending` either lands before the drain or re-schedules after it.
            let paths: Vec<String> = {
                let mut pending = pending.lock();
                let paths: Vec<String> = pending.drain().collect();
                flush_scheduled.store(false, Ordering::SeqCst);
                paths
            };
            if paths.is_empty() {
                return;
            }
            git.invalidate(&folder);
            tracing::trace!(folder = %folder.display(), ?paths, "git-changed");
            let _ = handle.emit(
                GIT_CHANGED_EVENT,
                GitChangedEvent {
                    folder_path: folder.to_string_lossy().into_owned(),
                    paths,
                },
            );
        });
    })
    .map_err(|error| format!("Failed to create git watcher: {error}"))?;

    watcher
        .watch(&git_dir, RecursiveMode::NonRecursive)
        .map_err(|error| format!("Failed to watch {}: {error}", git_dir.display()))?;
    let refs = common_dir.join("refs");
    if let Err(error) = watcher.watch(&refs, RecursiveMode::Recursive) {
        tracing::debug!(path = %refs.display(), %error, "git refs watch skipped");
    }
    Ok(watcher)
}

/// `path` relative to the git dir, or to the common dir for refs. `None`
/// when the path belongs to neither.
fn relative_git_path(git_dir: &Path, common_dir: &Path, path: &Path) -> Option<String> {
    let rel = path
        .strip_prefix(git_dir)
        .or_else(|_| path.strip_prefix(common_dir))
        .ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Files git touches without the repository state we surface having changed.
fn is_noise(rel: &str) -> bool {
    rel.ends_with(".lock")
        || rel.starts_with(".watchman-cookie-")
        || rel == "COMMIT_EDITMSG"
        || rel == "objects"
        || rel.starts_with("objects/")
        || rel == "logs"
        || rel.starts_with("logs/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_filter_keeps_state_files_only() {
        for noise in [
            "index.lock",
            ".watchman-cookie-x",
            "objects/ab",
            "COMMIT_EDITMSG",
            "logs/HEAD",
        ] {
            assert!(is_noise(noise), "{noise} should be noise");
        }
        for signal in [
            "index",
            "HEAD",
            "refs/heads/main",
            "FETCH_HEAD",
            "packed-refs",
        ] {
            assert!(!is_noise(signal), "{signal} should be kept");
        }
    }

    #[test]
    fn relative_git_path_handles_worktree_layout() {
        let git_dir = Path::new("/r/.git/worktrees/w");
        let common_dir = Path::new("/r/.git");

        assert_eq!(
            relative_git_path(git_dir, common_dir, Path::new("/r/.git/worktrees/w/index"))
                .as_deref(),
            Some("index")
        );
        assert_eq!(
            relative_git_path(git_dir, common_dir, Path::new("/r/.git/refs/heads/x")).as_deref(),
            Some("refs/heads/x")
        );
        assert_eq!(
            relative_git_path(git_dir, common_dir, Path::new("/elsewhere")),
            None
        );
    }
}
