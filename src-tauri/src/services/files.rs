use crate::errors::ApiError;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

const MAX_FILES: usize = 25_000;
const MAX_DEPTH: usize = 50;
/// Traversed after ordinary directories so generated trees do not exhaust the
/// explorer cap before user-authored files are discovered.
///
/// TODO: replace this heuristic with configurable hidden-file defaults in the
/// file explorer.
const LOW_PRIORITY_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".next",
    ".nuxt",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".cache",
    ".venv",
    "venv",
];

#[derive(Debug, Clone, Serialize)]
pub struct FilePasteCollision {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntryStat {
    pub is_dir: bool,
}

pub struct FileService;

impl FileService {
    pub fn new() -> Self {
        Self
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

    /// Return whether a project-relative path exists and is a directory.
    pub fn stat(
        &self,
        project_path: &Path,
        file_path: &str,
    ) -> Result<Option<FileEntryStat>, ApiError> {
        self.validate_path(file_path)?;
        let abs = project_path.join(file_path);
        let metadata = match std::fs::metadata(&abs) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(ApiError::Io(format!(
                    "Failed to stat {}: {}",
                    file_path, err
                )))
            }
        };

        Ok(Some(FileEntryStat {
            is_dir: metadata.is_dir(),
        }))
    }

    /// Paste files/directories into a target directory inside the project.
    /// `op` is "copy" or "cut". Sources are absolute paths (from clipboard).
    /// Returns the list of created relative paths (project-rooted).
    pub fn paste(
        &self,
        project_path: &Path,
        target_dir: &str,
        op: &str,
        sources: &[String],
        collision_policy: &str,
        rename_map: &HashMap<String, String>,
    ) -> Result<Vec<String>, ApiError> {
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

        let mut created: Vec<String> = Vec::new();

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
                created.push(rel.to_string_lossy().into_owned());
            }
        }

        Ok(created)
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

    /// List all files in the project.
    ///
    /// Filesystem-first: walks the real directory tree so the explorer reflects
    /// disk state (deleted files don't linger, newly created files appear
    /// immediately). No filtering — the explorer shows everything on disk —
    /// but bulky generated trees are traversed last so the 25k file cap is
    /// spent on likely-user-authored paths first. `.gitignore` is a git-tree
    /// concern and should only prune git views.
    pub fn list_all(&self, project_path: &Path) -> Result<Vec<String>, ApiError> {
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        self.walk_dir(project_path, "", &mut out, &mut visited, 0)?;
        out.sort();
        Ok(out)
    }

    /// Walk `dir` and push project-relative paths (prefixed with `rel_prefix`)
    /// into `out`. `rel_prefix` is the path of `dir` relative to the project
    /// root — "" when `dir` is the root itself. An explicit prefix (rather
    /// than `strip_prefix(root)`) lets us descend through a symlink whose
    /// target lives outside the project.
    ///
    /// Symlinks to directories are followed; `visited` tracks canonical
    /// targets so symlink cycles can't cause infinite recursion.
    fn walk_dir(
        &self,
        dir: &Path,
        rel_prefix: &str,
        out: &mut Vec<String>,
        visited: &mut HashSet<PathBuf>,
        depth: usize,
    ) -> Result<(), ApiError> {
        if depth > MAX_DEPTH || out.len() >= MAX_FILES {
            return Ok(());
        }

        let mut entries = std::fs::read_dir(dir)
            .map_err(|e| ApiError::Io(format!("Cannot read {}: {}", dir.display(), e)))?
            .map(|entry| {
                let entry = entry.map_err(|e| ApiError::Io(e.to_string()))?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                let ft = entry.file_type().map_err(|e| ApiError::Io(e.to_string()))?;
                Ok((is_low_priority_dir_name(&name), name, path, ft))
            })
            .collect::<Result<Vec<_>, ApiError>>()?;
        entries.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        for (_, name_str, path, ft) in entries {
            if out.len() >= MAX_FILES {
                return Ok(());
            }

            let rel_child = if rel_prefix.is_empty() {
                name_str
            } else {
                format!("{}/{}", rel_prefix, name_str)
            };

            // `file_type()` is lstat-based — for symlinks we need stat
            // (`metadata`) to see what the link actually points to.
            let target_is_dir = if ft.is_symlink() {
                std::fs::metadata(&path)
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
            } else {
                ft.is_dir()
            };

            if target_is_dir {
                if ft.is_symlink() {
                    if let Ok(canonical) = std::fs::canonicalize(&path) {
                        if !visited.insert(canonical) {
                            continue;
                        }
                    }
                }
                self.walk_dir(&path, &rel_child, out, visited, depth + 1)?;
            } else if ft.is_file() || ft.is_symlink() {
                out.push(rel_child);
            }
        }
        Ok(())
    }
}

fn is_low_priority_dir_name(name: &str) -> bool {
    LOW_PRIORITY_DIRS.contains(&name)
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
