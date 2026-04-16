use crate::errors::ApiError;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Directories always skipped during the walk, regardless of git status.
/// `.git` and build/cache dirs without a universally-agreed gitignore entry
/// are listed here so non-git projects (and git projects with a stale index)
/// still get a clean tree.
const SKIP_DIRS: &[&str] = &[
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

const MAX_FILES: usize = 25_000;
const MAX_DEPTH: usize = 50;

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

    /// Paste files/directories into a target directory inside the project.
    /// `op` is "copy" or "cut". Sources are absolute paths (from clipboard).
    /// Returns the list of created relative paths (project-rooted).
    pub fn paste(
        &self,
        project_path: &Path,
        target_dir: &str,
        op: &str,
        sources: &[String],
    ) -> Result<Vec<String>, ApiError> {
        if op != "copy" && op != "cut" {
            return Err(ApiError::InvalidArgument(format!("Invalid op: {}", op)));
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

            // Auto-rename if destination already exists.
            let dest_path = unique_path(&abs_target_dir.join(name));

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
    /// immediately). In a git repo we additionally query `.gitignore` once and
    /// prune matching paths during the walk — but git's index is never the
    /// source of truth for what exists.
    pub fn list_all(&self, project_path: &Path) -> Result<Vec<String>, ApiError> {
        let ignored = collect_ignored(project_path);
        let mut out = Vec::new();
        let mut visited = HashSet::new();
        self.walk_dir(project_path, "", &ignored, &mut out, &mut visited, 0)?;
        out.sort();
        Ok(out)
    }

    /// Walk `dir` and push project-relative paths (prefixed with `rel_prefix`)
    /// into `out`. `rel_prefix` is the path of `dir` relative to the project
    /// root — "" when `dir` is the root itself. An explicit prefix (rather
    /// than `strip_prefix(root)`) lets us descend through a symlink whose
    /// target lives outside the project.
    ///
    /// Skip rules, in order:
    ///   1. `SKIP_DIRS`  — always-hidden junk (`.git`, `node_modules`, …)
    ///   2. `ignored`    — paths matched by the project's `.gitignore`
    ///
    /// Dotted entries that pass both rules (`.github`, `.vscode`, `.agents`)
    /// are shown — matching how VS Code and other real file explorers behave.
    /// Symlinks to directories are followed; `visited` tracks canonical
    /// targets so symlink cycles can't cause infinite recursion.
    fn walk_dir(
        &self,
        dir: &Path,
        rel_prefix: &str,
        ignored: &HashSet<String>,
        out: &mut Vec<String>,
        visited: &mut HashSet<PathBuf>,
        depth: usize,
    ) -> Result<(), ApiError> {
        if depth > MAX_DEPTH || out.len() >= MAX_FILES {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)
            .map_err(|e| ApiError::Io(format!("Cannot read {}: {}", dir.display(), e)))?;

        for entry in entries {
            if out.len() >= MAX_FILES {
                return Ok(());
            }

            let entry = entry.map_err(|e| ApiError::Io(e.to_string()))?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let path = entry.path();
            let ft = entry.file_type().map_err(|e| ApiError::Io(e.to_string()))?;

            if SKIP_DIRS.contains(&&*name_str) {
                continue;
            }

            let rel_child = if rel_prefix.is_empty() {
                name_str.to_string()
            } else {
                format!("{}/{}", rel_prefix, name_str)
            };

            if ignored.contains(&rel_child) {
                continue;
            }

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
                self.walk_dir(&path, &rel_child, ignored, out, visited, depth + 1)?;
            } else if ft.is_file() || ft.is_symlink() {
                out.push(rel_child);
            }
        }
        Ok(())
    }
}

/// Ask git which paths it would ignore in this project. Returns an empty set
/// when the project isn't a git repo or git isn't available. `--directory`
/// collapses a wholly-ignored directory (e.g. `target/`, `dist/`) into one
/// entry so the walker can prune it instead of descending and filtering.
fn collect_ignored(project_path: &Path) -> HashSet<String> {
    let mut set = HashSet::new();
    let Ok(output) = Command::new("git")
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ])
        .current_dir(project_path)
        .output()
    else {
        return set;
    };
    if !output.status.success() {
        return set;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        set.insert(line.trim_end_matches('/').to_string());
    }
    set
}

// ── Paste helpers ─────────────────────────────────────────────────

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
