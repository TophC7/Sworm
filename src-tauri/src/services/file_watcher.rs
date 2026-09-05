//! Keeps the file explorer honest about disk state.
//!
//! Watches exactly the directories the explorer currently renders — the root
//! plus every expanded folder — non-recursively. That is deliberately narrow:
//! a recursive watch on a repo with a `node_modules` or `target` tree burns
//! thousands of inotify watches to report changes nobody can see.
//!
//! Events are coalesced per directory and emitted as `files-changed`, so the
//! frontend re-reads only the affected listings. VS Code debounces the
//! equivalent signal by 500ms; 300ms is fine here because the payload names
//! directories instead of forcing a whole-tree refresh.

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;

pub const FILES_CHANGED_EVENT: &str = "files-changed";

const FLUSH_DELAY: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Serialize)]
pub struct FilesChangedEvent {
    pub folder_path: String,
    /// Project-relative directories whose contents changed; "" is the root.
    pub dirs: Vec<String>,
}

struct FolderWatch {
    watcher: RecommendedWatcher,
    /// Shared with the event handler so it can drop events for directories the
    /// explorer no longer renders.
    watched: Arc<Mutex<HashSet<String>>>,
    window_requests: HashMap<String, HashSet<String>>,
}

pub struct FileWatcherService {
    folders: Mutex<HashMap<PathBuf, FolderWatch>>,
}

impl FileWatcherService {
    pub fn new() -> Self {
        Self {
            folders: Mutex::new(HashMap::new()),
        }
    }

    /// Update one window's requested directories. The underlying watcher sees
    /// the union, so one window cannot unwatch another window's expanded tree.
    pub fn sync(
        &self,
        app: &tauri::AppHandle,
        window_label: &str,
        folder: &Path,
        dirs: &[String],
    ) -> Result<(), String> {
        let folder = folder.to_path_buf();
        let mut folders = self.folders.lock();
        let entry = match folders.get_mut(&folder) {
            Some(entry) => entry,
            None => {
                let watch = spawn_watch(app, &folder)?;
                folders.insert(folder.clone(), watch);
                folders.get_mut(&folder).expect("watch just inserted")
            }
        };
        if let Some(existing) = entry.window_requests.get(window_label) {
            if existing.len() == dirs.len() && dirs.iter().all(|d| existing.contains(d)) {
                return Ok(());
            }
        }
        entry
            .window_requests
            .insert(window_label.to_string(), dirs.iter().cloned().collect());
        let desired = requested_union(&entry.window_requests);
        update_watched(&folder, entry, desired);
        Ok(())
    }

    pub fn release_window_folder(&self, window_label: &str, folder: &Path) {
        let mut folders = self.folders.lock();
        let should_remove = match folders.get_mut(folder) {
            Some(entry) => {
                entry.window_requests.remove(window_label);
                if entry.window_requests.is_empty() {
                    true
                } else {
                    let desired = requested_union(&entry.window_requests);
                    update_watched(folder, entry, desired);
                    false
                }
            }
            None => false,
        };
        if should_remove {
            folders.remove(folder);
        }
    }

    pub fn release_window(&self, window_label: &str) {
        self.folders.lock().retain(|folder, entry| {
            entry.window_requests.remove(window_label);
            if entry.window_requests.is_empty() {
                return false;
            }
            let desired = requested_union(&entry.window_requests);
            update_watched(folder, entry, desired);
            true
        });
    }
}

fn requested_union(window_requests: &HashMap<String, HashSet<String>>) -> HashSet<String> {
    window_requests
        .values()
        .flat_map(|dirs| dirs.iter().cloned())
        .collect()
}

fn update_watched(folder: &Path, entry: &mut FolderWatch, desired: HashSet<String>) {
    // The notify event loop locks `watched` from its callback, and
    // `watch`/`unwatch` block until that same thread answers — publish the new
    // set and release the lock before touching the watcher.
    let (stale, added) = {
        let mut watched = entry.watched.lock();
        let stale: Vec<String> = watched.difference(&desired).cloned().collect();
        let added: Vec<String> = desired.difference(&watched).cloned().collect();
        *watched = desired;
        (stale, added)
    };
    for stale in stale {
        let _ = entry.watcher.unwatch(&abs_dir(folder, &stale));
    }
    for added in added {
        let abs = abs_dir(folder, &added);
        if let Err(error) = entry.watcher.watch(&abs, RecursiveMode::NonRecursive) {
            tracing::debug!(path = %abs.display(), %error, "explorer watch skipped");
        }
    }
}

fn spawn_watch(app: &tauri::AppHandle, folder: &Path) -> Result<FolderWatch, String> {
    let watched = Arc::new(Mutex::new(HashSet::new()));
    let pending = Arc::new(Mutex::new(HashSet::<String>::new()));
    let flush_scheduled = Arc::new(AtomicBool::new(false));

    let handle = app.clone();
    let folder_for_events = folder.to_path_buf();
    let watched_for_events = Arc::clone(&watched);

    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        let touched: Vec<String> = {
            let watched = watched_for_events.lock();
            event
                .paths
                .iter()
                .filter_map(|path| changed_dir(&folder_for_events, path))
                .filter(|dir| watched.contains(dir))
                .collect()
        };
        if touched.is_empty() {
            return;
        }

        pending.lock().extend(touched);
        if flush_scheduled.swap(true, Ordering::SeqCst) {
            return;
        }

        let handle = handle.clone();
        let folder_path = folder_for_events.to_string_lossy().into_owned();
        let pending = Arc::clone(&pending);
        let flush_scheduled = Arc::clone(&flush_scheduled);
        std::thread::spawn(move || {
            std::thread::sleep(FLUSH_DELAY);
            // Re-arm under the `pending` lock: a producer that already added to
            // `pending` either lands before the drain or re-schedules after it.
            let dirs: Vec<String> = {
                let mut pending = pending.lock();
                let dirs: Vec<String> = pending.drain().collect();
                flush_scheduled.store(false, Ordering::SeqCst);
                dirs
            };
            if dirs.is_empty() {
                return;
            }
            let _ = handle.emit(FILES_CHANGED_EVENT, FilesChangedEvent { folder_path, dirs });
        });
    })
    .map_err(|error| format!("Failed to create explorer watcher: {error}"))?;

    Ok(FolderWatch {
        watcher,
        watched,
        window_requests: HashMap::new(),
    })
}

fn abs_dir(folder: &Path, rel_dir: &str) -> PathBuf {
    if rel_dir.is_empty() {
        folder.to_path_buf()
    } else {
        folder.join(rel_dir)
    }
}

/// The listing a changed path belongs to: its parent directory, relative to the
/// folder root. `None` when the path is outside the folder.
fn changed_dir(folder: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(folder).ok()?;
    let parent = rel.parent()?;
    Some(parent.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_dir_maps_paths_to_their_listing() {
        let folder = Path::new("/repo");

        assert_eq!(
            changed_dir(folder, Path::new("/repo/a.txt")).as_deref(),
            Some("")
        );
        assert_eq!(
            changed_dir(folder, Path::new("/repo/src/lib/a.ts")).as_deref(),
            Some("src/lib")
        );
        assert_eq!(changed_dir(folder, Path::new("/elsewhere/a.txt")), None);
    }

    #[test]
    fn test_per_window_watcher_union() {
        let mut requests = HashMap::new();
        requests.insert(
            "workbench-a".to_string(),
            ["src", "public"].into_iter().map(String::from).collect(),
        );
        requests.insert(
            "workbench-b".to_string(),
            ["src", "tests"].into_iter().map(String::from).collect(),
        );

        assert_eq!(
            requested_union(&requests),
            ["src", "public", "tests"]
                .into_iter()
                .map(String::from)
                .collect()
        );

        requests.remove("workbench-a");
        assert_eq!(
            requested_union(&requests),
            ["src", "tests"].into_iter().map(String::from).collect()
        );
    }
}
