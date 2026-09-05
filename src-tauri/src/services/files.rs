use crate::errors::ApiError;
#[cfg(test)]
use crate::models::settings::ExplorerSettings;
use crate::services::explorer_filter::{ExplorerFilter, IgnoreChain};
use crate::services::settings_resolution::resolve_effective_settings_for_folder_path;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const MAX_DEPTH: usize = 50;
/// Bounds the single-child chain a compacted row may represent, so a
/// pathological deep chain can't turn one listing into unbounded work.
const MAX_COMPACT_HOPS: usize = 32;
/// Ceiling for the flat search list behind Quick Open and the sidebar filter.
/// Search only — directory listings are unbounded — and hitting it is reported
/// to the UI rather than silently dropping paths.
const MAX_SEARCH_PATHS: usize = 200_000;

#[derive(Debug, Clone, Serialize)]
pub struct FilePasteCollision {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePasteMapping {
    pub source: String,
    pub destination: String,
}

/// One row in a directory listing.
#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    /// Display label. A compacted chain carries the whole run, e.g. "lib/utils".
    pub name: String,
    /// Project-relative path with forward slashes. For a compacted chain this
    /// is the deepest directory, which is also the key its children load under.
    pub path: String,
    pub is_dir: bool,
    /// Matched git's ignore rules — rendered dimmed.
    pub ignored: bool,
    /// Matched an `explorer.exclude` glob; only ever true when `show_hidden`.
    pub excluded: bool,
    /// Directories a compacted chain swallowed, excluding `path` itself. The
    /// explorer watches these too: a write inside one changes what the row
    /// should collapse to, and no other listing would report it.
    pub hops: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PathList {
    pub paths: Vec<String>,
    pub truncated: bool,
}

pub struct FileService {
    /// Compiled explorer filter per project, rebuilt when the settings
    /// generation moves so an edited `settings.jsonc` takes effect at once.
    filters: Mutex<HashMap<PathBuf, (u64, Arc<ExplorerFilter>)>>,
}

impl FileService {
    pub fn new() -> Self {
        Self {
            filters: Mutex::new(HashMap::new()),
        }
    }

    /// Reject paths that could escape the project root:
    /// - `..` components (traversal)
    /// - absolute paths (Unix RootDir, Windows Prefix) — `Path::join` discards
    ///   its base when given an absolute path, so these would bypass the root
    ///   and let callers read/write anywhere on disk.
    pub fn validate_path(&self, file_path: &str) -> Result<(), ApiError> {
        let has_escape = Path::new(file_path).components().any(|c| {
            matches!(
                c,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        });
        if has_escape {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid file path: {}",
                file_path
            )));
        }
        Ok(())
    }

    /// Read the contents of a file inside a project.
    pub fn read(&self, project_path: &Path, file_path: &str) -> Result<String, ApiError> {
        self.validate_path(file_path)?;
        let abs = project_path.join(file_path);
        std::fs::read_to_string(&abs)
            .map_err(|e| ApiError::Io(format!("Failed to read {}: {}", file_path, e)))
    }

    /// Write content to a file inside a project.
    /// The file must already exist or its parent directory must exist.
    pub fn write(
        &self,
        project_path: &Path,
        file_path: &str,
        content: &str,
    ) -> Result<(), ApiError> {
        self.validate_path(file_path)?;
        let abs = project_path.join(file_path);
        std::fs::write(&abs, content)
            .map_err(|e| ApiError::Io(format!("Failed to write {}: {}", file_path, e)))
    }

    /// Create a directory (and any missing parents) within a project.
    pub fn create_dir(&self, project_path: &Path, dir_path: &str) -> Result<(), ApiError> {
        self.validate_path(dir_path)?;
        let abs = project_path.join(dir_path);
        if abs.exists() {
            return Err(ApiError::InvalidArgument(format!(
                "Path already exists: {}",
                dir_path
            )));
        }
        std::fs::create_dir_all(&abs)
            .map_err(|e| ApiError::Io(format!("Failed to create directory {}: {}", dir_path, e)))
    }

    /// Rename (move) a file within a project.
    pub fn rename(
        &self,
        project_path: &Path,
        old_path: &str,
        new_path: &str,
    ) -> Result<(), ApiError> {
        self.validate_path(old_path)?;
        self.validate_path(new_path)?;
        let abs_old = project_path.join(old_path);
        let abs_new = project_path.join(new_path);
        if !abs_old.exists() {
            return Err(ApiError::NotFound(format!("File not found: {}", old_path)));
        }
        if abs_new.exists() {
            return Err(ApiError::InvalidArgument(format!(
                "Destination already exists: {}",
                new_path
            )));
        }
        // Ensure parent directory exists for the target path.
        if let Some(parent) = abs_new.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ApiError::Io(format!("Failed to create directory: {}", e)))?;
        }
        std::fs::rename(&abs_old, &abs_new).map_err(|e| {
            ApiError::Io(format!(
                "Failed to rename {} → {}: {}",
                old_path, new_path, e
            ))
        })
    }

    /// Paste files/directories into a target directory inside the project.
    /// `op` is "copy" or "cut". Sources are absolute paths (from clipboard).
    /// Returns created project-relative paths and their source mappings.
    pub fn paste(
        &self,
        project_path: &Path,
        target_dir: &str,
        op: &str,
        sources: &[String],
        collision_policy: &str,
        rename_map: &HashMap<String, String>,
    ) -> Result<Vec<FilePasteMapping>, ApiError> {
        if op != "copy" && op != "cut" {
            return Err(ApiError::InvalidArgument(format!("Invalid op: {}", op)));
        }
        if !matches!(
            collision_policy,
            "auto_rename" | "replace" | "skip" | "rename" | "error"
        ) {
            return Err(ApiError::InvalidArgument(format!(
                "Invalid collision policy: {}",
                collision_policy
            )));
        }
        self.validate_path(target_dir)?;
        let abs_target_dir = project_path.join(target_dir);
        if !abs_target_dir.exists() {
            return Err(ApiError::NotFound(format!(
                "Target directory not found: {}",
                target_dir
            )));
        }
        if !abs_target_dir.is_dir() {
            return Err(ApiError::InvalidArgument(format!(
                "Target is not a directory: {}",
                target_dir
            )));
        }

        let mut mappings = Vec::new();

        for source in sources {
            let src_path = Path::new(source);
            let name = src_path.file_name().ok_or_else(|| {
                ApiError::InvalidArgument(format!("Invalid source path: {}", source))
            })?;

            // For explicit rename resolution, allow the caller to choose
            // a different basename for this source.
            let mut desired_dest = abs_target_dir.join(name);
            if collision_policy == "rename" {
                if let Some(rename_to) = rename_map.get(source) {
                    validate_basename(rename_to)?;
                    desired_dest = abs_target_dir.join(rename_to);
                }
            }

            let dest_path = resolve_destination(
                project_path,
                src_path,
                source,
                &desired_dest,
                collision_policy,
                rename_map,
            )?;
            let Some(dest_path) = dest_path else {
                continue;
            };

            if op == "cut" && src_path == dest_path {
                continue;
            }

            if op == "cut" {
                // Try fast rename; fall back to copy+delete if it fails
                // (e.g. cross-filesystem moves).
                if std::fs::rename(src_path, &dest_path).is_err() {
                    copy_recursive(src_path, &dest_path)?;
                    remove_recursive(src_path)?;
                }
            } else {
                copy_recursive(src_path, &dest_path)?;
            }

            // Compute project-relative path for the created item
            if let Ok(rel) = dest_path.strip_prefix(project_path) {
                let destination = rel.to_string_lossy().into_owned();
                mappings.push(FilePasteMapping {
                    source: source.clone(),
                    destination,
                });
            }
        }

        Ok(mappings)
    }

    /// Return collisions for a paste/drop operation before transfer.
    pub fn paste_collisions(
        &self,
        project_path: &Path,
        target_dir: &str,
        sources: &[String],
    ) -> Result<Vec<FilePasteCollision>, ApiError> {
        self.validate_path(target_dir)?;
        let abs_target_dir = project_path.join(target_dir);
        if !abs_target_dir.exists() {
            return Err(ApiError::NotFound(format!(
                "Target directory not found: {}",
                target_dir
            )));
        }
        if !abs_target_dir.is_dir() {
            return Err(ApiError::InvalidArgument(format!(
                "Target is not a directory: {}",
                target_dir
            )));
        }

        let mut collisions = Vec::new();
        for source in sources {
            let src_path = Path::new(source);
            let name = src_path.file_name().ok_or_else(|| {
                ApiError::InvalidArgument(format!("Invalid source path: {}", source))
            })?;
            let dest_path = abs_target_dir.join(name);
            if !dest_path.exists() {
                continue;
            }

            let destination = match dest_path.strip_prefix(project_path) {
                Ok(rel) => rel.to_string_lossy().into_owned(),
                Err(_) => dest_path.to_string_lossy().into_owned(),
            };
            collisions.push(FilePasteCollision {
                source: source.clone(),
                destination,
            });
        }

        Ok(collisions)
    }

    /// Delete a file within a project.
    pub fn delete(&self, project_path: &Path, file_path: &str) -> Result<(), ApiError> {
        self.validate_path(file_path)?;
        let abs = project_path.join(file_path);
        if !abs.exists() {
            return Err(ApiError::NotFound(format!("File not found: {}", file_path)));
        }
        if abs.is_dir() {
            std::fs::remove_dir_all(&abs)
                .map_err(|e| ApiError::Io(format!("Failed to delete {}: {}", file_path, e)))
        } else {
            std::fs::remove_file(&abs)
                .map_err(|e| ApiError::Io(format!("Failed to delete {}: {}", file_path, e)))
        }
    }

    /// List one directory, the way the explorer renders it.
    ///
    /// Filesystem-first and lazy: a folder open reads only the root, and each
    /// expand reads exactly one more directory. There is no file cap, so no
    /// sibling can be silently dropped. Empty directories are listed like any
    /// other entry.
    ///
    /// `dir_path` is project-relative; "" is the project root.
    pub fn read_dir(
        &self,
        project_path: &Path,
        dir_path: &str,
        show_hidden: bool,
        generation: u64,
    ) -> Result<Vec<DirEntry>, ApiError> {
        self.validate_path(dir_path)?;
        let filter = self.filter(project_path, generation)?;
        read_dir_with_filter(project_path, dir_path, show_hidden, &filter)
    }

    /// Flat list of every searchable file path, for Quick Open and the sidebar
    /// filter. Prunes git-ignored paths (VS Code's `search.useIgnoreFiles`
    /// default) independently of the explorer's dim-vs-hide setting.
    pub fn list_paths(
        &self,
        project_path: &Path,
        show_hidden: bool,
        generation: u64,
    ) -> Result<PathList, ApiError> {
        let filter = self.filter(project_path, generation)?;
        Ok(list_paths_with_filter(
            project_path,
            show_hidden,
            &filter,
            MAX_SEARCH_PATHS,
        ))
    }

    /// Drop a closed project's compiled filter, whose per-directory gitignore
    /// cache would otherwise live for the rest of the process.
    pub fn evict(&self, project_path: &Path) {
        self.filters.lock().remove(project_path);
    }

    fn filter(
        &self,
        project_path: &Path,
        generation: u64,
    ) -> Result<Arc<ExplorerFilter>, ApiError> {
        let mut filters = self.filters.lock();
        if let Some((cached_generation, filter)) = filters.get(project_path) {
            if *cached_generation == generation {
                return Ok(Arc::clone(filter));
            }
        }

        let resolved = resolve_effective_settings_for_folder_path(Some(project_path))
            .map_err(|error| ApiError::Io(format!("Failed to resolve settings: {error}")))?;
        let filter = Arc::new(ExplorerFilter::build(
            project_path,
            &resolved.settings.explorer,
        )?);
        filters.insert(
            project_path.to_path_buf(),
            (generation, Arc::clone(&filter)),
        );
        Ok(filter)
    }
}

/// Entry survivors of one directory, before compaction.
struct RawEntry {
    name: String,
    /// Lowercased `name`, kept so sorting a large listing allocates once per
    /// entry instead of twice per comparison.
    sort_name: String,
    path: String,
    is_dir: bool,
    ignored: bool,
    excluded: bool,
}

fn read_dir_with_filter(
    project_path: &Path,
    dir_path: &str,
    show_hidden: bool,
    filter: &ExplorerFilter,
) -> Result<Vec<DirEntry>, ApiError> {
    // Children of an ignored directory are ignored too, and git never descends
    // to say so a second time.
    let parent_ignored = filter.is_ignored(dir_path, true);
    let mut entries = read_entries(
        project_path,
        dir_path,
        show_hidden,
        parent_ignored,
        filter,
        usize::MAX,
    )?;
    // Directories first, then case-insensitive alphabetical.
    entries.sort_by(|a, b| {
        (!a.is_dir, &a.sort_name, &a.name).cmp(&(!b.is_dir, &b.sort_name, &b.name))
    });

    Ok(entries
        .into_iter()
        .map(|entry| {
            if entry.is_dir && filter.compact_folders {
                compact(project_path, entry, show_hidden, filter)
            } else {
                DirEntry {
                    name: entry.name,
                    path: entry.path,
                    is_dir: entry.is_dir,
                    ignored: entry.ignored,
                    excluded: entry.excluded,
                    hops: Vec::new(),
                }
            }
        })
        .collect())
}

/// The entries of `dir_path` the explorer would render, at most `limit` of
/// them. Compaction only ever asks "exactly one child?", so it stops at two
/// rather than enumerating a `node_modules`-sized directory to find out.
fn read_entries(
    project_path: &Path,
    dir_path: &str,
    show_hidden: bool,
    parent_ignored: bool,
    filter: &ExplorerFilter,
    limit: usize,
) -> Result<Vec<RawEntry>, ApiError> {
    let abs = project_path.join(dir_path);
    // Every entry of this directory answers to the same ancestor `.gitignore`
    // chain, so it is resolved once rather than per entry.
    let chain = filter.ignore_chain(dir_path);
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&abs)
        .map_err(|e| ApiError::Io(format!("Cannot read {}: {}", abs.display(), e)))?
    {
        let entry = entry.map_err(|e| ApiError::Io(e.to_string()))?;
        if let Some(survivor) = survivor(
            &entry,
            dir_path,
            show_hidden,
            parent_ignored,
            filter,
            &chain,
        )? {
            out.push(survivor);
            if out.len() >= limit {
                break;
            }
        }
    }
    Ok(out)
}

/// One directory entry as the explorer sees it, or `None` when the exclude
/// globs or git's ignore rules hide it.
fn survivor(
    entry: &std::fs::DirEntry,
    dir_path: &str,
    show_hidden: bool,
    parent_ignored: bool,
    filter: &ExplorerFilter,
    chain: &IgnoreChain<'_>,
) -> Result<Option<RawEntry>, ApiError> {
    let name = entry.file_name().to_string_lossy().into_owned();
    let file_type = entry.file_type().map_err(|e| ApiError::Io(e.to_string()))?;
    // `file_type()` is lstat-based — for symlinks we need stat (`metadata`) to
    // see whether the link points at a directory.
    let is_dir = if file_type.is_symlink() {
        std::fs::metadata(entry.path())
            .map(|m| m.is_dir())
            .unwrap_or(false)
    } else {
        file_type.is_dir()
    };

    let path = if dir_path.is_empty() {
        name.clone()
    } else {
        format!("{dir_path}/{name}")
    };

    let excluded = filter.is_excluded(&path);
    if excluded && !show_hidden {
        return Ok(None);
    }
    let ignored = parent_ignored || chain.is_ignored(&path, is_dir);
    if ignored && filter.exclude_gitignore && !show_hidden {
        return Ok(None);
    }

    Ok(Some(RawEntry {
        sort_name: name.to_lowercase(),
        name,
        path,
        is_dir,
        ignored,
        excluded,
    }))
}

/// Collapse a chain of single-child directories into one row, matching VS
/// Code's `explorer.compactFolders`. A directory that can't be listed stops the
/// descent and is reported uncompacted.
fn compact(
    project_path: &Path,
    entry: RawEntry,
    show_hidden: bool,
    filter: &ExplorerFilter,
) -> DirEntry {
    let mut row = DirEntry {
        name: entry.name,
        path: entry.path,
        is_dir: true,
        ignored: entry.ignored,
        excluded: entry.excluded,
        hops: Vec::new(),
    };

    for _ in 0..MAX_COMPACT_HOPS {
        let Ok(mut children) =
            read_entries(project_path, &row.path, show_hidden, row.ignored, filter, 2)
        else {
            break;
        };
        if children.len() != 1 || !children[0].is_dir {
            break;
        }
        let only = children.remove(0);
        row.name = format!("{}/{}", row.name, only.name);
        row.hops.push(std::mem::replace(&mut row.path, only.path));
        row.ignored = only.ignored;
        row.excluded = only.excluded;
    }

    row
}

fn list_paths_with_filter(
    project_path: &Path,
    show_hidden: bool,
    filter: &Arc<ExplorerFilter>,
    limit: usize,
) -> PathList {
    let root = project_path.to_path_buf();
    let prune_root = root.clone();
    let prune_filter = Arc::clone(filter);
    let mut walker = ignore::WalkBuilder::new(project_path);
    walker
        // Dotfiles are legitimate project files; `.git` is pruned below.
        .hidden(false)
        .parents(true)
        .follow_links(true)
        .max_depth(Some(MAX_DEPTH))
        .git_ignore(!show_hidden)
        .git_global(!show_hidden)
        .git_exclude(!show_hidden)
        .require_git(true)
        // Pruning here (rather than filtering results) is what keeps excluded
        // trees from being walked at all.
        .filter_entry(move |entry| {
            let Ok(rel) = entry.path().strip_prefix(&prune_root) else {
                return true;
            };
            // Pruning `.git` at its own level means no descendant is ever
            // visited, so only this entry's own name needs checking.
            if rel.file_name().is_some_and(|name| name == ".git") {
                return false;
            }
            let rel = rel.to_string_lossy().replace('\\', "/");
            show_hidden || rel.is_empty() || !prune_filter.is_excluded(&rel)
        });

    let mut paths = Vec::new();
    let mut truncated = false;
    for entry in walker.build() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                tracing::debug!(%error, "skipping unreadable path during search walk");
                continue;
            }
        };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            continue;
        }
        if paths.len() >= limit {
            truncated = true;
            break;
        }
        let Ok(rel) = entry.path().strip_prefix(&root) else {
            continue;
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        paths.push(rel.to_string_lossy().replace('\\', "/"));
    }

    paths.sort();
    PathList { paths, truncated }
}

// ── Paste helpers ─────────────────────────────────────────────────

fn resolve_destination(
    project_path: &Path,
    src_path: &Path,
    source_key: &str,
    desired_dest: &Path,
    collision_policy: &str,
    rename_map: &HashMap<String, String>,
) -> Result<Option<PathBuf>, ApiError> {
    if !desired_dest.exists() {
        return Ok(Some(desired_dest.to_path_buf()));
    }

    match collision_policy {
        "auto_rename" => Ok(Some(unique_path(desired_dest))),
        "replace" => {
            if desired_dest == src_path {
                return Ok(Some(desired_dest.to_path_buf()));
            }
            remove_recursive(desired_dest)?;
            Ok(Some(desired_dest.to_path_buf()))
        }
        "skip" => Ok(None),
        "rename" => {
            let rename_to = rename_map.get(source_key).ok_or_else(|| {
                ApiError::InvalidArgument(format!(
                    "Rename policy requires rename_map entry for source: {}",
                    source_key
                ))
            })?;
            validate_basename(rename_to)?;
            let parent = desired_dest.parent().unwrap_or(project_path);
            let candidate = parent.join(rename_to);
            if candidate == src_path {
                return Ok(Some(candidate));
            }
            if candidate.exists() {
                return Err(ApiError::InvalidArgument(format!(
                    "Destination already exists: {}",
                    candidate.display()
                )));
            }
            Ok(Some(candidate))
        }
        "error" => Err(ApiError::InvalidArgument(format!(
            "Destination already exists: {}",
            desired_dest.display()
        ))),
        other => Err(ApiError::InvalidArgument(format!(
            "Invalid collision policy: {}",
            other
        ))),
    }
}

fn validate_basename(value: &str) -> Result<(), ApiError> {
    if value.trim().is_empty() {
        return Err(ApiError::InvalidArgument(
            "Rename value cannot be empty".into(),
        ));
    }
    if value == "." || value == ".." || value.contains('/') || value.contains('\\') {
        return Err(ApiError::InvalidArgument(format!(
            "Invalid rename value: {}",
            value
        )));
    }
    Ok(())
}

/// Return a non-colliding path by appending " (copy)", " (copy 2)", etc.
fn unique_path(desired: &Path) -> std::path::PathBuf {
    if !desired.exists() {
        return desired.to_path_buf();
    }
    let parent = desired.parent().unwrap_or_else(|| Path::new(""));
    let stem = desired
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = desired
        .extension()
        .map(|s| s.to_string_lossy().into_owned());

    for i in 1..1000 {
        let name = if i == 1 {
            match &ext {
                Some(e) => format!("{} (copy).{}", stem, e),
                None => format!("{} (copy)", stem),
            }
        } else {
            match &ext {
                Some(e) => format!("{} (copy {}).{}", stem, i, e),
                None => format!("{} (copy {})", stem, i),
            }
        };
        let candidate = parent.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    desired.to_path_buf()
}

/// Recursively copy a file or directory. Capped at `MAX_DEPTH` to prevent
/// runaway recursion on symlink loops or pathological bind mounts.
fn copy_recursive(src: &Path, dest: &Path) -> Result<(), ApiError> {
    copy_recursive_bounded(src, dest, 0)
}

fn copy_recursive_bounded(src: &Path, dest: &Path, depth: usize) -> Result<(), ApiError> {
    if depth > MAX_DEPTH {
        return Err(ApiError::Io(format!(
            "Copy aborted: max depth {} exceeded at {}",
            MAX_DEPTH,
            src.display()
        )));
    }
    let metadata = std::fs::symlink_metadata(src)
        .map_err(|e| ApiError::Io(format!("Cannot stat {}: {}", src.display(), e)))?;

    if metadata.is_dir() {
        std::fs::create_dir_all(dest)
            .map_err(|e| ApiError::Io(format!("Cannot create {}: {}", dest.display(), e)))?;
        let entries = std::fs::read_dir(src)
            .map_err(|e| ApiError::Io(format!("Cannot read {}: {}", src.display(), e)))?;
        for entry in entries {
            let entry = entry.map_err(|e| ApiError::Io(e.to_string()))?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            copy_recursive_bounded(&child_src, &child_dest, depth + 1)?;
        }
    } else {
        std::fs::copy(src, dest).map_err(|e| {
            ApiError::Io(format!(
                "Cannot copy {} -> {}: {}",
                src.display(),
                dest.display(),
                e
            ))
        })?;
    }
    Ok(())
}

/// Recursively remove a file or directory.
fn remove_recursive(path: &Path) -> Result<(), ApiError> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| ApiError::Io(format!("Cannot stat {}: {}", path.display(), e)))?;
    if metadata.is_dir() {
        std::fs::remove_dir_all(path)
            .map_err(|e| ApiError::Io(format!("Cannot remove {}: {}", path.display(), e)))
    } else {
        std::fs::remove_file(path)
            .map_err(|e| ApiError::Io(format!("Cannot remove {}: {}", path.display(), e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_test_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "sworm-files-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    fn touch(path: PathBuf) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, "").expect("write file");
    }

    /// Filters are built directly so tests never depend on an on-disk
    /// `settings.jsonc`.
    fn filter(root: &Path, exclude_gitignore: bool, compact_folders: bool) -> Arc<ExplorerFilter> {
        let settings = ExplorerSettings {
            exclude: BTreeMap::from([("**/.git".to_string(), true)]),
            exclude_gitignore,
            compact_folders,
        };
        Arc::new(ExplorerFilter::build(root, &settings).expect("build filter"))
    }

    fn names(entries: &[DirEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.name.as_str()).collect()
    }

    #[test]
    fn read_dir_lists_empty_directories() {
        let dir = unique_test_dir("empty-dirs");
        std::fs::create_dir_all(dir.join("empty")).expect("empty dir");
        touch(dir.join("a.txt"));

        let entries =
            read_dir_with_filter(&dir, "", false, &filter(&dir, false, false)).expect("read dir");

        assert_eq!(names(&entries), vec!["empty", "a.txt"]);
        assert!(entries[0].is_dir);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_hides_git_by_default_and_reveals_it_with_show_hidden() {
        let dir = unique_test_dir("git-row");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, false, false);
        let hidden = read_dir_with_filter(&dir, "", false, &filter).expect("read dir");
        assert_eq!(names(&hidden), vec!["src"]);

        let shown = read_dir_with_filter(&dir, "", true, &filter).expect("read dir");
        assert_eq!(names(&shown), vec![".git", "src"]);
        assert!(shown[0].excluded);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_compacts_single_child_chains() {
        let dir = unique_test_dir("compact");
        touch(dir.join("src/lib/utils/a.ts"));

        let compacted =
            read_dir_with_filter(&dir, "", false, &filter(&dir, false, true)).expect("read dir");
        assert_eq!(names(&compacted), vec!["src/lib/utils"]);
        assert_eq!(compacted[0].path, "src/lib/utils");
        // The swallowed directories: watching these is the only way a write
        // inside them can split the row back apart.
        assert_eq!(compacted[0].hops, vec!["src", "src/lib"]);

        touch(dir.join("src/other.ts"));
        let split =
            read_dir_with_filter(&dir, "", false, &filter(&dir, false, true)).expect("read dir");
        assert_eq!(names(&split), vec!["src"]);
        assert_eq!(split[0].path, "src");
        assert!(split[0].hops.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_marks_gitignored_entries_without_hiding_them() {
        let dir = unique_test_dir("ignored-dim");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        std::fs::write(dir.join(".gitignore"), "gen/\n").expect("ignore file");
        touch(dir.join("gen/out.js"));
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, false, false);
        let root = read_dir_with_filter(&dir, "", false, &filter).expect("read dir");
        assert_eq!(names(&root), vec!["gen", "src", ".gitignore"]);
        let gen = root
            .iter()
            .find(|entry| entry.name == "gen")
            .expect("gen row");
        assert!(gen.ignored);
        assert!(
            !root
                .iter()
                .find(|entry| entry.name == "src")
                .expect("src row")
                .ignored
        );

        // Children inherit the parent's ignored state; git does not repeat itself.
        let children = read_dir_with_filter(&dir, "gen", false, &filter).expect("read dir");
        assert!(children[0].ignored);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_dir_hides_gitignored_entries_when_configured() {
        let dir = unique_test_dir("ignored-hide");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        std::fs::write(dir.join(".gitignore"), "gen/\n").expect("ignore file");
        touch(dir.join("gen/out.js"));
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, true, false);
        let hidden = read_dir_with_filter(&dir, "", false, &filter).expect("read dir");
        assert_eq!(names(&hidden), vec!["src", ".gitignore"]);

        let shown = read_dir_with_filter(&dir, "", true, &filter).expect("read dir");
        assert_eq!(names(&shown), vec![".git", "gen", "src", ".gitignore"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_paths_prunes_git_and_gitignored_paths() {
        let dir = unique_test_dir("list-paths");
        std::fs::create_dir_all(dir.join(".git")).expect("fake git dir");
        touch(dir.join(".git/objects/deadbeef"));
        std::fs::write(dir.join(".gitignore"), "gen/\n").expect("ignore file");
        touch(dir.join("gen/out.js"));
        touch(dir.join("src/main.rs"));

        let filter = filter(&dir, false, false);
        let listed = list_paths_with_filter(&dir, false, &filter, MAX_SEARCH_PATHS);
        assert_eq!(listed.paths, vec![".gitignore", "src/main.rs"]);
        assert!(!listed.truncated);

        // show_hidden keeps ignored paths searchable but never walks `.git`.
        let all = list_paths_with_filter(&dir, true, &filter, MAX_SEARCH_PATHS);
        assert_eq!(all.paths, vec![".gitignore", "gen/out.js", "src/main.rs"]);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn list_paths_flags_truncation() {
        let dir = unique_test_dir("truncation");
        for index in 0..3 {
            touch(dir.join(format!("file-{index}.txt")));
        }

        let filter = filter(&dir, false, false);
        let mut listed = list_paths_with_filter(&dir, false, &filter, MAX_SEARCH_PATHS);
        assert!(!listed.truncated);

        listed = list_paths_with_filter(&dir, false, &filter, 2);
        assert!(listed.truncated);
        assert_eq!(listed.paths.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }
}
