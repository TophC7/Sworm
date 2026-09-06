//! Keeps the git sidebar honest about repository and working-tree state.
//!
//! Each open folder gets non-recursive watches for Git-aware directories
//! across its repository worktree plus metadata watches for its per-worktree
//! git dir and common refs. Ignored trees are pruned, except directories
//! containing tracked files.

use crate::services::git::GitService;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::Emitter;

pub const GIT_CHANGED_EVENT: &str = "git-changed";

const QUIET_DELAY: Duration = Duration::from_millis(150);
const MAX_FLUSH_DELAY: Duration = Duration::from_millis(750);

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

type EventSink = Arc<dyn Fn(GitChangedEvent) + Send + Sync>;

struct FolderWatch {
    control: mpsc::Sender<WorkerMessage>,
    worker: Option<JoinHandle<()>>,
    healthy: Arc<AtomicBool>,
}

impl FolderWatch {
    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }
}

impl Drop for FolderWatch {
    fn drop(&mut self) {
        self.healthy.store(false, Ordering::Release);
        let _ = self.control.send(WorkerMessage::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub struct GitWatcherService {
    folders: Mutex<HashMap<PathBuf, FolderWatch>>,
    git: Arc<GitService>,
}

impl GitWatcherService {
    pub fn new(git: Arc<GitService>) -> Self {
        Self {
            folders: Mutex::new(HashMap::new()),
            git,
        }
    }

    /// Start watching `folder`; healthy watches are idempotent and failed
    /// workers are removed so a later call can retry.
    pub fn watch(&self, app: &tauri::AppHandle, folder: &Path) -> Result<(), String> {
        // Serialize setup with `stop`: otherwise a folder release could observe
        // no entry while setup is in flight, then leave the late worker alive.
        let mut folders = self.folders.lock();
        if folders.get(folder).is_some_and(FolderWatch::is_healthy) {
            return Ok(());
        }
        drop(folders.remove(folder));

        let handle = app.clone();
        let sink: EventSink = Arc::new(move |event| {
            let _ = handle.emit(GIT_CHANGED_EVENT, event);
        });
        let watch = match start_folder_watch(Arc::clone(&self.git), folder, Arc::clone(&sink)) {
            Ok(watch) => watch,
            Err(error) => {
                emit_change(
                    &self.git,
                    folder,
                    &sink,
                    GitChangeScope::All,
                    Some(error.clone()),
                );
                return Err(error);
            }
        };

        tracing::debug!(folder = %folder.display(), "git watcher started");
        folders.insert(folder.to_path_buf(), watch);
        Ok(())
    }

    pub fn stop(&self, folder: &Path) {
        let watch = self.folders.lock().remove(folder);
        drop(watch);
    }
}

enum WorkerMessage {
    Notify(notify::Result<notify::Event>),
    Stop,
}

fn flush_deadline(first: Instant, last: Instant) -> Instant {
    std::cmp::min(first + MAX_FLUSH_DELAY, last + QUIET_DELAY)
}

fn start_folder_watch(
    git: Arc<GitService>,
    folder: &Path,
    sink: EventSink,
) -> Result<FolderWatch, String> {
    let (worktree_root, git_dir, common_dir) = resolve_git_layout(folder)?;
    let mut worktree_dirs = collect_worktree_dirs(&worktree_root)?;
    let (control, events) = mpsc::channel();
    let callback = control.clone();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = callback.send(WorkerMessage::Notify(event));
    })
    .map_err(|error| format!("Failed to create git watcher: {error}"))?;

    for dir in &worktree_dirs {
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", dir.display()))?;
    }
    let metadata_roots: HashSet<PathBuf> =
        [git_dir.clone(), common_dir.clone()].into_iter().collect();
    for dir in &metadata_roots {
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", dir.display()))?;
    }
    let refs = common_dir.join("refs");
    if refs.is_dir() {
        watcher
            .watch(&refs, RecursiveMode::Recursive)
            .map_err(|error| format!("Failed to watch {}: {error}", refs.display()))?;
    }

    let folder = folder.to_path_buf();
    let healthy = Arc::new(AtomicBool::new(true));
    let worker_health = Arc::clone(&healthy);
    let worker = std::thread::Builder::new()
        .name("git-watcher".to_string())
        .spawn(move || {
            run_worker(
                watcher,
                events,
                git,
                folder,
                worktree_root,
                git_dir,
                common_dir,
                refs,
                &mut worktree_dirs,
                sink,
                &worker_health,
            );
        })
        .map_err(|error| format!("Failed to start git watcher worker: {error}"))?;

    Ok(FolderWatch {
        control,
        worker: Some(worker),
        healthy,
    })
}

#[allow(clippy::too_many_arguments)]
fn run_worker(
    mut watcher: RecommendedWatcher,
    events: mpsc::Receiver<WorkerMessage>,
    git: Arc<GitService>,
    folder: PathBuf,
    worktree_root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
    refs: PathBuf,
    worktree_dirs: &mut HashSet<PathBuf>,
    sink: EventSink,
    healthy: &AtomicBool,
) {
    let mut pending_scope = None;
    let mut pending_paths = HashSet::new();
    let mut first_change = None;
    let mut last_change = None;
    let mut structural_change = false;
    let mut force_coverage_refresh = false;
    let mut refresh_refs = false;

    loop {
        if !healthy.load(Ordering::Acquire) {
            break;
        }
        let now = Instant::now();
        let flush_at = first_change
            .zip(last_change)
            .map(|(first, last)| flush_deadline(first, last));
        let timeout = flush_at
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(Duration::from_secs(24 * 60 * 60));

        match events.recv_timeout(timeout) {
            Ok(WorkerMessage::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(WorkerMessage::Notify(Err(error))) => {
                fail_worker(
                    &git,
                    &folder,
                    &sink,
                    healthy,
                    format!("Git watcher failed: {error}"),
                );
                return;
            }
            Ok(WorkerMessage::Notify(Ok(event))) => {
                if matches!(&event.kind, EventKind::Access(_)) {
                    continue;
                }
                let (scope, paths) =
                    classify_event_paths(&worktree_root, &git_dir, &common_dir, &event);
                if scope.is_none() && paths.is_empty() {
                    continue;
                }
                if let Some(scope) = scope {
                    pending_scope = Some(widest_scope(pending_scope, scope));
                }
                pending_paths.extend(paths);
                structural_change |= changes_directory_structure(worktree_dirs, &event);
                force_coverage_refresh |= changes_index(&git_dir, &event);
                force_coverage_refresh |= changes_ignore_rules(&worktree_root, &event);
                refresh_refs |= changes_ref_coverage(&refs, &event);
                let now = Instant::now();
                first_change.get_or_insert(now);
                last_change = Some(now);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        let now = Instant::now();
        if flush_at.is_some_and(|deadline| now >= deadline) {
            let paths: Vec<PathBuf> = pending_paths.drain().collect();
            let worktree_changed =
                match has_relevant_worktree_path(&worktree_root, worktree_dirs, &paths) {
                    Ok(relevant) => relevant,
                    Err(error) => {
                        fail_worker(&git, &folder, &sink, healthy, error);
                        return;
                    }
                };
            if force_coverage_refresh || (structural_change && worktree_changed) {
                if let Err(error) =
                    replace_worktree_watches(&mut watcher, &worktree_root, worktree_dirs)
                {
                    fail_worker(&git, &folder, &sink, healthy, error);
                    return;
                }
            }
            if refresh_refs {
                let _ = watcher.unwatch(&refs);
                if refs.is_dir() {
                    if let Err(error) = watcher.watch(&refs, RecursiveMode::Recursive) {
                        fail_worker(
                            &git,
                            &folder,
                            &sink,
                            healthy,
                            format!("Failed to watch {}: {error}", refs.display()),
                        );
                        return;
                    }
                }
            }
            if worktree_changed {
                pending_scope = Some(widest_scope(pending_scope, GitChangeScope::Summary));
            }
            if let Some(scope) = pending_scope.take() {
                emit_change(&git, &folder, &sink, scope, None);
            }
            first_change = None;
            last_change = None;
            structural_change = false;
            force_coverage_refresh = false;
            refresh_refs = false;
        }
    }

    healthy.store(false, Ordering::Release);
}

fn fail_worker(
    git: &GitService,
    folder: &Path,
    sink: &EventSink,
    healthy: &AtomicBool,
    error: String,
) {
    healthy.store(false, Ordering::Release);
    tracing::warn!(folder = %folder.display(), %error, "git watcher stopped");
    emit_change(git, folder, sink, GitChangeScope::All, Some(error));
}

fn emit_change(
    git: &GitService,
    folder: &Path,
    sink: &EventSink,
    scope: GitChangeScope,
    error: Option<String>,
) {
    git.invalidate(folder);
    sink(GitChangedEvent {
        folder_path: folder.to_string_lossy().into_owned(),
        scope,
        error,
    });
}

fn widest_scope(current: Option<GitChangeScope>, next: GitChangeScope) -> GitChangeScope {
    if current == Some(GitChangeScope::All) || next == GitChangeScope::All {
        GitChangeScope::All
    } else {
        GitChangeScope::Summary
    }
}

/// `(worktree_root, git_dir, common_dir)`; linked worktrees keep HEAD/index
/// in the per-worktree git dir and refs plus packed-refs in the common dir.
fn resolve_git_layout(folder: &Path) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let output = Command::new("git")
        .args([
            "--no-optional-locks",
            "rev-parse",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ])
        .current_dir(folder)
        .output()
        .map_err(|error| format!("Failed to run git: {error}"))?;
    if !output.status.success() {
        return Err(git_failure("resolve repository metadata", &output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    let worktree_root = lines
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| "Git returned no worktree root".to_string())?;
    let git_dir = lines
        .next()
        .map(|line| absolute(folder, line))
        .ok_or_else(|| "Git returned no metadata directory".to_string())?;
    let common_dir = lines
        .next()
        .map(|line| absolute(folder, line))
        .unwrap_or_else(|| git_dir.clone());
    Ok((worktree_root, git_dir, common_dir))
}

fn absolute(folder: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        folder.join(path)
    }
}

/// Non-ignored directories plus existing ancestors of every tracked file.
/// `git ls-files` is essential: a tracked file remains visible even when a
/// later ignore rule matches it.
fn collect_worktree_dirs(folder: &Path) -> Result<HashSet<PathBuf>, String> {
    let tracked = git_output(folder, &["ls-files", "-z"], "list tracked files")?;
    let mut dirs = HashSet::from([folder.to_path_buf()]);
    for raw in tracked
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let relative = PathBuf::from(String::from_utf8_lossy(raw).into_owned());
        let mut parent = relative.parent();
        while let Some(dir) = parent {
            let absolute = folder.join(dir);
            if absolute.is_dir() {
                dirs.insert(absolute);
            }
            parent = dir.parent();
        }
    }

    let root = folder.to_path_buf();
    let mut walk = ignore::WalkBuilder::new(folder);
    walk.hidden(false)
        .parents(true)
        .follow_links(false)
        .ignore(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(true)
        .filter_entry(move |entry| {
            entry.path() == root
                || !entry
                    .file_type()
                    .is_some_and(|kind| kind.is_dir() && entry.file_name() == ".git")
        });
    for entry in walk.build() {
        let entry = entry.map_err(|error| format!("Failed to scan Git worktree: {error}"))?;
        if entry.file_type().is_some_and(|kind| kind.is_dir()) {
            dirs.insert(entry.into_path());
        }
    }
    Ok(dirs)
}

fn replace_worktree_watches(
    watcher: &mut RecommendedWatcher,
    folder: &Path,
    watched: &mut HashSet<PathBuf>,
) -> Result<(), String> {
    let desired = collect_worktree_dirs(folder)?;
    // Re-register every path, not only set differences. Atomic directory
    // replacement keeps the pathname but invalidates an inode-backed watch.
    for path in watched.drain() {
        let _ = watcher.unwatch(&path);
    }
    for path in &desired {
        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", path.display()))?;
    }
    *watched = desired;
    Ok(())
}

fn classify_event_paths(
    worktree_root: &Path,
    git_dir: &Path,
    common_dir: &Path,
    event: &notify::Event,
) -> (Option<GitChangeScope>, Vec<PathBuf>) {
    let mut worktree = Vec::new();
    let mut scope = None;
    for path in &event.paths {
        if let Some(relative) = relative_git_path(git_dir, common_dir, path) {
            if is_metadata_noise(&relative) {
                continue;
            }
            let path_scope = if relative == "index" {
                GitChangeScope::Summary
            } else {
                GitChangeScope::All
            };
            scope = Some(widest_scope(scope, path_scope));
        } else if path.starts_with(worktree_root) {
            worktree.push(path.clone());
        }
    }
    (scope, worktree)
}

fn has_relevant_worktree_path(
    folder: &Path,
    watched: &HashSet<PathBuf>,
    paths: &[PathBuf],
) -> Result<bool, String> {
    let mut candidates = Vec::new();
    for path in paths {
        if watched.iter().any(|dir| dir.starts_with(path)) {
            return Ok(true);
        }
        if let Ok(relative) = path.strip_prefix(folder) {
            if !relative.as_os_str().is_empty() {
                candidates.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    if candidates.is_empty() {
        return Ok(false);
    }

    let mut child = Command::new("git")
        .args(["--no-optional-locks", "check-ignore", "--stdin", "-z"])
        .current_dir(folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to run git check-ignore: {error}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "Failed to open git check-ignore stdin".to_string())?;
        for path in &candidates {
            stdin
                .write_all(path.as_bytes())
                .and_then(|_| stdin.write_all(&[0]))
                .map_err(|error| format!("Failed to query git ignores: {error}"))?;
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|error| format!("Failed to query git ignores: {error}"))?;
    if !output.status.success() && output.status.code() != Some(1) {
        return Err(git_failure("query ignore rules", &output));
    }
    let ignored: HashSet<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
        .map(|raw| String::from_utf8_lossy(raw).into_owned())
        .collect();
    Ok(candidates.iter().any(|path| !ignored.contains(path)))
}

fn changes_directory_structure(watched: &HashSet<PathBuf>, event: &notify::Event) -> bool {
    matches!(
        &event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
    ) && event
        .paths
        .iter()
        .any(|path| path.is_dir() || watched.contains(path))
}

fn changes_ignore_rules(worktree_root: &Path, event: &notify::Event) -> bool {
    event.paths.iter().any(|path| {
        (path.starts_with(worktree_root)
            && path.file_name().is_some_and(|name| name == ".gitignore"))
            || (path.file_name().is_some_and(|name| name == "exclude")
                && path
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "info"))
    })
}

fn changes_index(git_dir: &Path, event: &notify::Event) -> bool {
    event
        .paths
        .iter()
        .filter_map(|path| path.strip_prefix(git_dir).ok())
        .any(|relative| relative == Path::new("index"))
}

fn changes_ref_coverage(refs: &Path, event: &notify::Event) -> bool {
    matches!(
        &event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
    ) && event.paths.iter().any(|path| path == refs)
}
fn relative_git_path(git_dir: &Path, common_dir: &Path, path: &Path) -> Option<String> {
    let relative = path
        .strip_prefix(git_dir)
        .or_else(|_| path.strip_prefix(common_dir))
        .ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

fn is_metadata_noise(relative: &str) -> bool {
    relative.ends_with(".lock")
        || relative.starts_with(".watchman-cookie-")
        || relative == "COMMIT_EDITMSG"
        || relative == "objects"
        || relative.starts_with("objects/")
        || relative == "logs"
        || relative.starts_with("logs/")
}

fn git_output(folder: &Path, args: &[&str], action: &str) -> Result<Vec<u8>, String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(args)
        .current_dir(folder)
        .output()
        .map_err(|error| format!("Failed to {action}: {error}"))?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(git_failure(action, &output))
    }
}

fn git_failure(action: &str, output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("Failed to {action}: git exited with {}", output.status)
    } else {
        format!("Failed to {action}: {detail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sworm-git-watcher-{tag}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path).unwrap();
        git(&path, &["init", "-q"]);
        path
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn recv_summary(receiver: &std::sync::mpsc::Receiver<GitChangedEvent>) {
        let event = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("receive git change");
        assert_eq!(event.scope, GitChangeScope::Summary);
        assert_eq!(event.error, None);
    }

    #[test]
    fn worktree_coverage_prunes_ignored_dirs_but_keeps_tracked_ignored_dirs() {
        let repo = temp_repo("coverage");
        std::fs::write(repo.join(".gitignore"), "build/\nlocked/\n").unwrap();
        std::fs::write(repo.join(".ignore"), "scratch/\n").unwrap();
        std::fs::create_dir_all(repo.join("build/deep")).unwrap();
        std::fs::create_dir_all(repo.join("locked/deep")).unwrap();
        std::fs::create_dir_all(repo.join("scratch")).unwrap();
        std::fs::write(repo.join("locked/deep/kept.txt"), "tracked").unwrap();
        git(&repo, &["add", "-f", "locked/deep/kept.txt"]);

        let dirs = collect_worktree_dirs(&repo).unwrap();
        assert!(!dirs.contains(&repo.join("build")));
        assert!(!dirs.contains(&repo.join("build/deep")));
        assert!(
            dirs.contains(&repo.join("scratch")),
            ".ignore is not a Git rule and must not prune watcher coverage"
        );
        assert!(dirs.contains(&repo.join("locked")));
        assert!(dirs.contains(&repo.join("locked/deep")));

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn watcher_setup_surfaces_git_failures() {
        let repo = temp_repo("setup-error");
        std::fs::write(repo.join(".git/index"), b"broken index").unwrap();
        let sink: EventSink = Arc::new(|_| {});

        assert!(
            start_folder_watch(Arc::new(GitService::new()), &repo, sink).is_err(),
            "broken Git metadata must fail watcher setup"
        );

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn ignore_filter_keeps_tracked_paths() {
        let repo = temp_repo("ignore");
        std::fs::write(repo.join(".gitignore"), "*.log\n").unwrap();
        std::fs::write(repo.join("ignored.log"), "ignored").unwrap();
        std::fs::write(repo.join("tracked.log"), "tracked").unwrap();
        git(&repo, &["add", "-f", "tracked.log"]);
        let watched = HashSet::from([repo.clone()]);

        assert!(!has_relevant_worktree_path(&repo, &watched, &[repo.join("ignored.log")]).unwrap());
        assert!(has_relevant_worktree_path(&repo, &watched, &[repo.join("tracked.log")]).unwrap());

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn watcher_reports_nested_writes_and_rewatches_new_and_replaced_dirs() {
        let repo = temp_repo("events");
        std::fs::write(repo.join(".gitignore"), "build/\nlocked/\n").unwrap();
        std::fs::create_dir_all(repo.join("nested/deep")).unwrap();
        std::fs::create_dir_all(repo.join("build")).unwrap();
        std::fs::create_dir_all(repo.join("locked")).unwrap();
        std::fs::write(repo.join("nested/deep/file with space.txt"), "old").unwrap();
        std::fs::write(repo.join("locked/kept.txt"), "old").unwrap();
        git(
            &repo,
            &[
                "add",
                "-f",
                ".gitignore",
                "nested/deep/file with space.txt",
                "locked/kept.txt",
            ],
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        let sink: EventSink = Arc::new(move |event| {
            sender.send(event).unwrap();
        });
        let watch =
            start_folder_watch(Arc::new(GitService::new()), &repo, sink).expect("start watcher");

        std::fs::write(repo.join("nested/deep/file with space.txt"), "edited").unwrap();
        recv_summary(&receiver);

        std::fs::write(repo.join("build/churn.txt"), "ignored").unwrap();
        assert!(
            receiver.recv_timeout(Duration::from_millis(400)).is_err(),
            "ignored build churn must not refresh Git"
        );

        std::fs::write(repo.join("locked/kept.txt"), "tracked ignored edit").unwrap();
        recv_summary(&receiver);

        std::fs::create_dir_all(repo.join("new/deep")).unwrap();
        std::fs::write(repo.join("new/deep/first.txt"), "created").unwrap();
        recv_summary(&receiver);
        std::fs::write(repo.join("new/deep/second.txt"), "created later").unwrap();
        recv_summary(&receiver);

        std::fs::create_dir_all(repo.join("replacement")).unwrap();
        std::fs::write(repo.join("replacement/base.txt"), "replacement").unwrap();
        std::fs::rename(repo.join("nested"), repo.join("old-nested")).unwrap();
        std::fs::rename(repo.join("replacement"), repo.join("nested")).unwrap();
        recv_summary(&receiver);
        std::fs::write(repo.join("nested/after.txt"), "after replacement").unwrap();
        recv_summary(&receiver);

        drop(watch);
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn selected_subdirectory_watches_the_repository_root() {
        let repo = temp_repo("subdirectory");
        let selected = repo.join("selected");
        let sibling = repo.join("sibling");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();

        let (sender, receiver) = std::sync::mpsc::channel();
        let sink: EventSink = Arc::new(move |event| {
            sender.send(event).unwrap();
        });
        let watch = start_folder_watch(Arc::new(GitService::new()), &selected, sink)
            .expect("start watcher from subdirectory");

        std::fs::write(sibling.join("outside-selected.txt"), "changed").unwrap();
        let event = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("repository-root change");
        assert_eq!(event.folder_path, selected.to_string_lossy().into_owned());
        assert_eq!(event.scope, GitChangeScope::Summary);

        drop(watch);
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn metadata_scope_handles_linked_worktree_layout() {
        let git_dir = Path::new("/repo/.git/worktrees/linked");
        let common_dir = Path::new("/repo/.git");

        assert_eq!(
            relative_git_path(
                git_dir,
                common_dir,
                Path::new("/repo/.git/worktrees/linked/index")
            )
            .as_deref(),
            Some("index")
        );
        assert_eq!(
            relative_git_path(git_dir, common_dir, Path::new("/repo/.git/packed-refs")).as_deref(),
            Some("packed-refs")
        );
    }

    #[test]
    fn continuous_writes_have_bounded_flush_latency() {
        let first = Instant::now();
        assert_eq!(
            flush_deadline(first, first + Duration::from_secs(10)),
            first + MAX_FLUSH_DELAY
        );
        assert_eq!(
            flush_deadline(first, first + Duration::from_millis(20)),
            first + Duration::from_millis(20) + QUIET_DELAY
        );
    }
}
