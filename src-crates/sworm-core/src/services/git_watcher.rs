//! Keeps the git sidebar honest about repository and working-tree state.
//!
//! Each open folder gets non-recursive watches for Git-aware directories
//! across its repository worktree plus metadata watches for its per-worktree
//! git dir, common refs and shared worktree registrations. Ignored trees are
//! pruned, except directories containing tracked files.

use crate::events::{EventSink, HostEvent};
use crate::services::git::{git_output, git_output_line, git_with_stdin, GitService};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use sworm_protocol::git::{GitChangeScope, GitChangedEvent};

const QUIET_DELAY: Duration = Duration::from_millis(150);
const MAX_FLUSH_DELAY: Duration = Duration::from_millis(750);

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
    /// workers are removed so a later call can retry. Setup runs outside the
    /// map lock so one large repository does not stall other folders; `owned`
    /// is re-checked under the lock so a folder released mid-setup never keeps
    /// a live watcher (callers drop claims before calling `stop`).
    pub fn watch(
        &self,
        events: EventSink<HostEvent>,
        folder: &Path,
        owned: impl FnOnce() -> bool,
    ) -> Result<(), String> {
        {
            let mut folders = self.folders.lock();
            if folders.get(folder).is_some_and(FolderWatch::is_healthy) {
                return Ok(());
            }
            drop(folders.remove(folder));
        }

        let watch = match start_folder_watch(Arc::clone(&self.git), folder, Arc::clone(&events)) {
            Ok(watch) => watch,
            Err(error) => {
                emit_change(
                    &self.git,
                    folder,
                    &events,
                    GitChangeScope::All,
                    Some(error.clone()),
                );
                return Err(error);
            }
        };

        let mut folders = self.folders.lock();
        if !owned() {
            tracing::debug!(folder = %folder.display(), "git watcher discarded: folder released");
            return Ok(());
        }
        if folders.get(folder).is_some_and(FolderWatch::is_healthy) {
            return Ok(());
        }
        tracing::debug!(folder = %folder.display(), "git watcher started");
        drop(folders.insert(folder.to_path_buf(), watch));
        Ok(())
    }

    pub fn stop(&self, folder: &Path) {
        let watch = self.folders.lock().remove(folder);
        drop(watch);
    }

    pub fn watched_folders(&self) -> Vec<PathBuf> {
        self.folders.lock().keys().cloned().collect()
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
    sink: EventSink<HostEvent>,
) -> Result<FolderWatch, String> {
    let (worktree_root, git_dir, common_dir) = resolve_git_layout(folder)?;
    let mut coverage = collect_worktree_coverage(&worktree_root)?;
    let (control, events) = mpsc::channel();
    let callback = control.clone();
    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = callback.send(WorkerMessage::Notify(event));
    })
    .map_err(|error| format!("Failed to create git watcher: {error}"))?;

    for dir in &coverage.dirs {
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", dir.display()))?;
    }
    // `info` holds the repository-local `exclude` rules.
    let info = common_dir.join("info");
    let metadata_roots: HashSet<PathBuf> = [git_dir.clone(), common_dir.clone()]
        .into_iter()
        .chain(info.is_dir().then_some(info))
        .collect();
    for dir in &metadata_roots {
        watcher
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", dir.display()))?;
    }
    let metadata_trees = [common_dir.join("refs"), common_dir.join("worktrees")];
    for dir in &metadata_trees {
        if dir.is_dir() {
            watcher
                .watch(dir, RecursiveMode::Recursive)
                .map_err(|error| format!("Failed to watch {}: {error}", dir.display()))?;
        }
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
                metadata_trees,
                &mut coverage,
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
    metadata_trees: [PathBuf; 2],
    coverage: &mut WorktreeCoverage,
    sink: EventSink<HostEvent>,
    healthy: &AtomicBool,
) {
    let mut pending_scope = None;
    let mut pending_paths = HashSet::new();
    let mut first_change = None;
    let mut last_change = None;
    let mut structural_change = false;
    let mut index_changed = false;
    let mut ignore_changed = false;
    let mut refresh_metadata = [false; 2];

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
                if loses_metadata_root(&git_dir, &common_dir, &event) {
                    fail_worker(
                        &git,
                        &folder,
                        &sink,
                        healthy,
                        "Git metadata directory was removed or replaced".to_string(),
                    );
                    return;
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
                structural_change |= changes_directory_structure(&coverage.dirs, &event);
                index_changed |= changes_index(&git_dir, &event);
                ignore_changed |= changes_ignore_rules(&worktree_root, &event);
                for (dir, refresh) in metadata_trees.iter().zip(&mut refresh_metadata) {
                    *refresh |= changes_metadata_coverage(dir, &event);
                }
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
                match has_relevant_worktree_path(&worktree_root, &coverage.dirs, &paths) {
                    Ok(relevant) => relevant,
                    Err(error) => {
                        fail_worker(&git, &folder, &sink, healthy, error);
                        return;
                    }
                };
            let rebuild = structural_change && worktree_changed;
            if rebuild || index_changed || ignore_changed {
                let plan = if rebuild {
                    CoveragePlan::Rebuild
                } else if ignore_changed {
                    CoveragePlan::Diff
                } else {
                    CoveragePlan::DiffIfTrackedChanged
                };
                if let Err(error) =
                    replace_worktree_watches(&mut watcher, &worktree_root, coverage, plan)
                {
                    fail_worker(&git, &folder, &sink, healthy, error);
                    return;
                }
            }
            for (dir, refresh) in metadata_trees.iter().zip(&mut refresh_metadata) {
                if *refresh {
                    let _ = watcher.unwatch(dir);
                    if dir.is_dir() {
                        if let Err(error) = watcher.watch(dir, RecursiveMode::Recursive) {
                            fail_worker(
                                &git,
                                &folder,
                                &sink,
                                healthy,
                                format!("Failed to watch {}: {error}", dir.display()),
                            );
                            return;
                        }
                    }
                    *refresh = false;
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
            index_changed = false;
            ignore_changed = false;
        }
    }

    healthy.store(false, Ordering::Release);
}

fn fail_worker(
    git: &GitService,
    folder: &Path,
    sink: &EventSink<HostEvent>,
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
    sink: &EventSink<HostEvent>,
    scope: GitChangeScope,
    error: Option<String>,
) {
    git.invalidate(folder);
    let _ = sink(HostEvent::GitChanged(GitChangedEvent {
        folder_path: folder.to_string_lossy().into_owned(),
        scope,
        error,
    }));
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
    // Separate outputs preserve embedded newlines; Git's single trailing LF
    // is removed by the shared helper without trimming path whitespace.
    let resolve = |option| {
        let path = PathBuf::from(git_output_line(
            folder,
            &["rev-parse", "--path-format=absolute", option],
            "resolve repository metadata",
        )?);
        if !path.is_absolute() {
            return Err(format!("Git returned a non-absolute path for {option}"));
        }
        Ok(path)
    };
    Ok((
        resolve("--show-toplevel")?,
        resolve("--git-dir")?,
        resolve("--git-common-dir")?,
    ))
}

/// Watched worktree directories plus a fingerprint of the tracked-file list
/// that produced them, so index churn without tracked-set changes is free.
struct WorktreeCoverage {
    dirs: HashSet<PathBuf>,
    tracked_hash: u64,
}

enum CoveragePlan {
    /// Re-register every path: atomic directory replacement keeps the
    /// pathname but invalidates an inode-backed watch.
    Rebuild,
    /// Ignore rules changed; watch only the set difference.
    Diff,
    /// Index changed; skip entirely unless the tracked-file list moved.
    DiffIfTrackedChanged,
}

fn collect_worktree_coverage(folder: &Path) -> Result<WorktreeCoverage, String> {
    let tracked = git_output(folder, &["ls-files", "-z"], "list tracked files")?;
    Ok(WorktreeCoverage {
        dirs: collect_worktree_dirs(folder, &tracked)?,
        tracked_hash: hash_bytes(&tracked),
    })
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

fn tracked_path(raw: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(raw.to_vec()))
    }
    #[cfg(not(unix))]
    {
        PathBuf::from(String::from_utf8_lossy(raw).into_owned())
    }
}

/// Non-ignored directories plus existing ancestors of every tracked file.
/// `git ls-files` is essential: a tracked file remains visible even when a
/// later ignore rule matches it.
fn collect_worktree_dirs(folder: &Path, tracked: &[u8]) -> Result<HashSet<PathBuf>, String> {
    let mut dirs = HashSet::from([folder.to_path_buf()]);
    for raw in tracked
        .split(|byte| *byte == 0)
        .filter(|raw| !raw.is_empty())
    {
        let relative = tracked_path(raw);
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
    coverage: &mut WorktreeCoverage,
    plan: CoveragePlan,
) -> Result<(), String> {
    let tracked = git_output(folder, &["ls-files", "-z"], "list tracked files")?;
    let tracked_hash = hash_bytes(&tracked);
    if matches!(plan, CoveragePlan::DiffIfTrackedChanged) && tracked_hash == coverage.tracked_hash {
        return Ok(());
    }
    let desired = collect_worktree_dirs(folder, &tracked)?;
    let watched = &mut coverage.dirs;
    if matches!(plan, CoveragePlan::Rebuild) {
        for path in watched.drain() {
            let _ = watcher.unwatch(&path);
        }
    } else {
        watched.retain(|path| {
            let keep = desired.contains(path);
            if !keep {
                let _ = watcher.unwatch(path);
            }
            keep
        });
    }
    for path in desired.difference(watched) {
        watcher
            .watch(path, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", path.display()))?;
    }
    *coverage = WorktreeCoverage {
        dirs: desired,
        tracked_hash,
    };
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
            if is_metadata_noise(relative)
                || (!path.starts_with(git_dir) && is_primary_worktree_metadata(relative))
            {
                continue;
            }
            let path_scope = if relative == Path::new("index") {
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
        if watched.contains(path) {
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

    let mut input = Vec::new();
    for path in &candidates {
        input.extend_from_slice(path.as_bytes());
        input.push(0);
    }
    let output = git_with_stdin(folder, &["check-ignore", "--stdin", "-z"], &input)?;
    let ignored: HashSet<String> = output
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

/// The watched metadata directory itself vanished or was renamed; its inode
/// watch is dead, so the worker must fail and let the frontend re-arm.
fn loses_metadata_root(git_dir: &Path, common_dir: &Path, event: &notify::Event) -> bool {
    matches!(
        &event.kind,
        EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
    ) && event
        .paths
        .iter()
        .any(|path| path == git_dir || path == common_dir)
}

fn changes_metadata_coverage(root: &Path, event: &notify::Event) -> bool {
    matches!(
        &event.kind,
        EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(notify::event::ModifyKind::Name(_))
    ) && event.paths.iter().any(|path| path == root)
}

fn relative_git_path<'a>(git_dir: &Path, common_dir: &Path, path: &'a Path) -> Option<&'a Path> {
    path.strip_prefix(git_dir)
        .or_else(|_| path.strip_prefix(common_dir))
        .ok()
}

fn is_metadata_noise(relative: &Path) -> bool {
    if let Ok(registration) = relative.strip_prefix("worktrees") {
        let mut components = registration.components();
        // The directory itself and each registration affect inventory. Below
        // a sibling registration only these files change its displayed state.
        components.next();
        return match (components.next(), components.next()) {
            (None, _) => false,
            (Some(std::path::Component::Normal(name)), None) => {
                name != "HEAD" && name != "gitdir" && name != "locked"
            }
            _ => true,
        };
    }
    relative.file_name().is_some_and(|name| {
        let name = name.as_encoded_bytes();
        name.ends_with(b".lock") || name.starts_with(b".watchman-cookie-")
    }) || relative == Path::new("COMMIT_EDITMSG")
        || relative.starts_with("objects")
        || relative.starts_with("logs")
}

/// A linked checkout sees the primary Git directory as common metadata, but
/// its index and operation state belong to the primary checkout, not this one.
fn is_primary_worktree_metadata(relative: &Path) -> bool {
    relative.components().next().is_some_and(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some(
                "index"
                    | "ORIG_HEAD"
                    | "MERGE_HEAD"
                    | "MERGE_MSG"
                    | "MERGE_MODE"
                    | "AUTO_MERGE"
                    | "REBASE_HEAD"
                    | "CHERRY_PICK_HEAD"
                    | "REVERT_HEAD"
                    | "REVERT_MSG"
                    | "SQUASH_MSG"
                    | "rebase-merge"
                    | "rebase-apply"
                    | "sequencer"
            )
        ) || component
            .as_os_str()
            .to_str()
            .is_some_and(|name| name.starts_with("BISECT_"))
    }) || relative.starts_with("refs/bisect")
        || relative.starts_with("refs/rewritten")
        || relative.starts_with("refs/worktree")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

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

    fn committed_repo(tag: &str) -> PathBuf {
        let repo = temp_repo(tag);
        std::fs::write(repo.join("tracked.txt"), "tracked\n").unwrap();
        git(&repo, &["add", "tracked.txt"]);
        for message in ["first", "second"] {
            git(
                &repo,
                &[
                    "-c",
                    "user.name=Sworm Watcher",
                    "-c",
                    "user.email=watcher@example.invalid",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    message,
                ],
            );
        }
        repo
    }

    fn watch_repo(repo: &Path) -> (FolderWatch, mpsc::Receiver<HostEvent>) {
        let (sender, receiver) = mpsc::channel();
        let sink: EventSink<HostEvent> =
            Arc::new(move |event| sender.send(event).map_err(|error| error.to_string()));
        let watch =
            start_folder_watch(Arc::new(GitService::new()), repo, sink).expect("start watcher");
        (watch, receiver)
    }

    fn recv_scope(receiver: &mpsc::Receiver<HostEvent>, scope: GitChangeScope) -> GitChangedEvent {
        let HostEvent::GitChanged(event) = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("receive git change")
        else {
            panic!("received non-Git host event");
        };
        assert_eq!(event.scope, scope);
        assert_eq!(event.error, None);
        event
    }

    fn recv_summary(receiver: &mpsc::Receiver<HostEvent>) {
        recv_scope(receiver, GitChangeScope::Summary);
    }

    fn assert_quiet(receiver: &mpsc::Receiver<HostEvent>) {
        assert!(
            matches!(
                receiver.recv_timeout(MAX_FLUSH_DELAY + QUIET_DELAY),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "sibling index, log and operation churn must not emit a Git change"
        );
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

        let dirs = collect_worktree_coverage(&repo).unwrap().dirs;
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

    #[cfg(unix)]
    #[test]
    fn tracked_non_utf8_paths_keep_their_ignored_parent_watched() {
        use std::os::unix::ffi::OsStringExt;
        let repo = temp_repo("non-utf8");
        std::fs::write(repo.join(".gitignore"), "locked/\n").unwrap();
        let dir = repo
            .join("locked")
            .join(std::ffi::OsString::from_vec(b"caf\xe9".to_vec()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kept.txt"), "tracked").unwrap();
        git(&repo, &["add", "-f", "locked"]);

        let dirs = collect_worktree_coverage(&repo).unwrap().dirs;
        assert!(
            dirs.contains(&dir),
            "lossy path decoding drops the tracked directory"
        );

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn replacing_git_dir_fails_the_worker_so_it_can_rearm() {
        let repo = temp_repo("replaced-git");
        let (sender, receiver) = std::sync::mpsc::channel();
        let sink: EventSink<HostEvent> =
            Arc::new(move |event| sender.send(event).map_err(|error| error.to_string()));
        let watch =
            start_folder_watch(Arc::new(GitService::new()), &repo, sink).expect("start watcher");

        std::fs::rename(repo.join(".git"), repo.join(".git-old")).unwrap();
        let HostEvent::GitChanged(event) = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("metadata loss event")
        else {
            panic!("received non-Git host event");
        };
        assert!(
            event.error.is_some(),
            "worker must report the dead metadata watch"
        );
        assert!(!watch.is_healthy());

        drop(watch);
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn index_churn_without_tracked_changes_keeps_watches() {
        let repo = temp_repo("index-churn");
        std::fs::write(repo.join("a.txt"), "a").unwrap();
        git(&repo, &["add", "a.txt"]);
        let mut coverage = collect_worktree_coverage(&repo).unwrap();
        let before = coverage.tracked_hash;
        let mut watcher = notify::recommended_watcher(|_| {}).unwrap();

        std::fs::create_dir_all(repo.join("later")).unwrap();
        replace_worktree_watches(
            &mut watcher,
            &repo,
            &mut coverage,
            CoveragePlan::DiffIfTrackedChanged,
        )
        .unwrap();
        assert_eq!(coverage.tracked_hash, before);
        assert!(
            !coverage.dirs.contains(&repo.join("later")),
            "unchanged index skips rescan"
        );

        std::fs::write(repo.join("later/b.txt"), "b").unwrap();
        git(&repo, &["add", "later/b.txt"]);
        replace_worktree_watches(
            &mut watcher,
            &repo,
            &mut coverage,
            CoveragePlan::DiffIfTrackedChanged,
        )
        .unwrap();
        assert_ne!(coverage.tracked_hash, before);
        assert!(coverage.dirs.contains(&repo.join("later")));

        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn watcher_setup_surfaces_git_failures() {
        let repo = temp_repo("setup-error");
        std::fs::write(repo.join(".git/index"), b"broken index").unwrap();
        let sink: EventSink<HostEvent> = Arc::new(|_| Ok(()));

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
        let sink: EventSink<HostEvent> =
            Arc::new(move |event| sender.send(event).map_err(|error| error.to_string()));
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
        let sink: EventSink<HostEvent> =
            Arc::new(move |event| sender.send(event).map_err(|error| error.to_string()));
        let watch = start_folder_watch(Arc::new(GitService::new()), &selected, sink)
            .expect("start watcher from subdirectory");

        std::fs::write(sibling.join("outside-selected.txt"), "changed").unwrap();
        let HostEvent::GitChanged(event) = receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("repository-root change")
        else {
            panic!("received non-Git host event");
        };
        assert_eq!(event.folder_path, selected.to_string_lossy().into_owned());
        assert_eq!(event.scope, GitChangeScope::Summary);

        drop(watch);
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn watcher_layout_preserves_whitespace_in_primary_and_linked_paths() {
        let original = committed_repo("layout-whitespace");
        let repo = original.with_extension(" primary\twith\nspace ");
        let linked = original.with_extension(" linked\twith\nspace ");
        std::fs::rename(&original, &repo).unwrap();
        let selected = repo.join(" selected\twith\nspace ");
        std::fs::create_dir(&selected).unwrap();
        let expected_root = std::fs::canonicalize(&repo).unwrap();
        let expected_common = std::fs::canonicalize(repo.join(".git")).unwrap();
        assert_eq!(
            resolve_git_layout(&selected).unwrap(),
            (
                expected_root,
                expected_common.clone(),
                expected_common.clone()
            )
        );
        let (primary_watch, primary_events) = watch_repo(&selected);

        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        let event = recv_scope(&primary_events, GitChangeScope::All);
        assert_eq!(event.folder_path, selected.to_str().unwrap());
        let linked_selected = linked.join(" selected\twith\nspace ");
        std::fs::create_dir(&linked_selected).unwrap();
        let (root, git_dir, common_dir) = resolve_git_layout(&linked_selected).unwrap();
        assert_eq!(root, std::fs::canonicalize(&linked).unwrap());
        assert_eq!(common_dir, expected_common);
        assert!(git_dir.starts_with(common_dir.join("worktrees")));
        assert!(git_dir.join("HEAD").is_file());
        let (linked_watch, linked_events) = watch_repo(&linked_selected);

        git(&repo, &["checkout", "-q", "--detach", "HEAD~1"]);
        recv_scope(&primary_events, GitChangeScope::All);
        recv_scope(&linked_events, GitChangeScope::All);
        std::fs::write(linked.join("tracked.txt"), "linked edit\n").unwrap();
        let event = recv_scope(&linked_events, GitChangeScope::Summary);
        assert_eq!(event.folder_path, linked_selected.to_str().unwrap());
        assert_quiet(&primary_events);

        drop(primary_watch);
        drop(linked_watch);
        std::fs::remove_dir_all(linked).ok();
        std::fs::remove_dir_all(repo).ok();
    }

    #[cfg(unix)]
    #[test]
    fn watcher_layout_rejects_non_utf8_repository_paths() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let original = temp_repo("layout-non-utf8");
        let mut bytes = original.as_os_str().as_bytes().to_vec();
        bytes.push(0xff);
        let repo = PathBuf::from(std::ffi::OsString::from_vec(bytes));
        std::fs::rename(&original, &repo).unwrap();
        let error = resolve_git_layout(&repo).unwrap_err();
        assert!(
            error.contains("UTF-8"),
            "must reject, not repair a path: {error}"
        );
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn primary_watcher_tracks_external_worktrees_and_recreated_registration_root() {
        let repo = committed_repo("shared-worktrees");
        let linked = repo.with_extension("linked");
        let moved = repo.with_extension("moved");
        let registrations = repo.join(".git/worktrees");
        assert!(!registrations.exists());
        let (watch, receiver) = watch_repo(&repo);

        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        let event = recv_scope(&receiver, GitChangeScope::All);
        assert_eq!(event.folder_path, repo.to_str().unwrap());

        // Detached HEAD changes do not update shared branch refs. They must
        // reach a watcher with only the primary checkout open.
        git(&linked, &["checkout", "-q", "--detach", "HEAD~1"]);
        recv_scope(&receiver, GitChangeScope::All);

        std::fs::rename(&linked, &moved).unwrap();
        git(&repo, &["worktree", "repair", moved.to_str().unwrap()]);
        recv_scope(&receiver, GitChangeScope::All);

        git(
            &repo,
            &[
                "worktree",
                "lock",
                "--reason",
                "keep offline",
                moved.to_str().unwrap(),
            ],
        );
        recv_scope(&receiver, GitChangeScope::All);
        git(&repo, &["worktree", "unlock", moved.to_str().unwrap()]);
        recv_scope(&receiver, GitChangeScope::All);
        git(&repo, &["worktree", "remove", moved.to_str().unwrap()]);
        if registrations.exists() {
            std::fs::remove_dir(&registrations).unwrap();
        }
        recv_scope(&receiver, GitChangeScope::All);
        assert!(!registrations.exists());

        git(
            &repo,
            &[
                "worktree",
                "add",
                "--detach",
                linked.to_str().unwrap(),
                "HEAD",
            ],
        );
        recv_scope(&receiver, GitChangeScope::All);
        git(&linked, &["checkout", "-q", "--detach", "HEAD~1"]);
        recv_scope(&receiver, GitChangeScope::All);

        std::fs::remove_dir_all(&linked).unwrap();
        git(&repo, &["worktree", "prune", "--expire=now"]);
        recv_scope(&receiver, GitChangeScope::All);
        assert!(watch.is_healthy(), "registration loss must rearm, not stop");

        drop(watch);
        std::fs::remove_dir_all(repo).ok();
    }

    #[test]
    fn shared_metadata_ignores_sibling_churn_but_keeps_own_and_primary_head_events() {
        let repo = committed_repo("shared-noise");
        let linked = repo.with_extension("linked");
        let sibling = repo.with_extension("sibling");
        for path in [&linked, &sibling] {
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "--detach",
                    path.to_str().unwrap(),
                    "HEAD",
                ],
            );
        }
        let (_, primary_git, _) = resolve_git_layout(&repo).unwrap();
        let (_, own_git, _) = resolve_git_layout(&linked).unwrap();
        let (_, sibling_git, _) = resolve_git_layout(&sibling).unwrap();
        let (primary_watch, primary_events) = watch_repo(&repo);
        let (linked_watch, linked_events) = watch_repo(&linked);

        for file in ["index", "logs/HEAD"] {
            let path = sibling_git.join(file);
            std::fs::write(&path, std::fs::read(&path).unwrap()).unwrap();
        }
        std::fs::write(sibling_git.join("HEAD.lock"), "transient").unwrap();
        std::fs::remove_file(sibling_git.join("HEAD.lock")).unwrap();
        std::fs::write(sibling_git.join("MERGE_HEAD"), "operation").unwrap();
        std::fs::create_dir(sibling_git.join("rebase-merge")).unwrap();
        std::fs::write(sibling_git.join("rebase-merge/head-name"), "operation").unwrap();
        assert_quiet(&primary_events);
        assert_quiet(&linked_events);

        let index = primary_git.join("index");
        std::fs::write(&index, std::fs::read(&index).unwrap()).unwrap();
        recv_summary(&primary_events);
        assert_quiet(&linked_events);
        std::fs::write(primary_git.join("MERGE_HEAD"), "operation").unwrap();
        recv_scope(&primary_events, GitChangeScope::All);
        assert_quiet(&linked_events);
        std::fs::remove_file(primary_git.join("MERGE_HEAD")).unwrap();
        recv_scope(&primary_events, GitChangeScope::All);
        assert_quiet(&linked_events);
        git(
            &repo,
            &["update-ref", "refs/rewritten/watcher-test", "HEAD"],
        );
        recv_scope(&primary_events, GitChangeScope::All);
        assert_quiet(&linked_events);

        let index = own_git.join("index");
        std::fs::write(&index, std::fs::read(&index).unwrap()).unwrap();
        recv_summary(&linked_events);
        assert_quiet(&primary_events);
        std::fs::write(own_git.join("MERGE_HEAD"), "operation").unwrap();
        recv_scope(&linked_events, GitChangeScope::All);
        assert_quiet(&primary_events);
        git(
            &linked,
            &["update-ref", "refs/rewritten/watcher-test", "HEAD"],
        );
        recv_scope(&linked_events, GitChangeScope::All);
        assert_quiet(&primary_events);

        let head = sibling_git.join("HEAD");
        std::fs::write(&head, std::fs::read(&head).unwrap()).unwrap();
        recv_scope(&primary_events, GitChangeScope::All);
        recv_scope(&linked_events, GitChangeScope::All);
        git(&repo, &["checkout", "-q", "--detach", "HEAD~1"]);
        recv_scope(&primary_events, GitChangeScope::All);
        recv_scope(&linked_events, GitChangeScope::All);
        assert!(primary_watch.is_healthy());
        assert!(linked_watch.is_healthy());

        drop(primary_watch);
        drop(linked_watch);
        std::fs::remove_dir_all(linked).ok();
        std::fs::remove_dir_all(sibling).ok();
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
            ),
            Some(Path::new("index"))
        );
        assert_eq!(
            relative_git_path(git_dir, common_dir, Path::new("/repo/.git/packed-refs")),
            Some(Path::new("packed-refs"))
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
