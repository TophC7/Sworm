use crate::errors::ApiError;
use std::path::{Component, Path};
use std::process::Command;

/// Directories to skip when walking a non-git project.
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

    /// Reject file paths containing `..` components (path traversal).
    pub fn validate_path(&self, file_path: &str) -> Result<(), ApiError> {
        if Path::new(file_path)
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
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

    /// List all files in the project.
    /// Uses git ls-files when in a git repo, falls back to filesystem walk otherwise.
    pub fn list_all(&self, project_path: &Path) -> Result<Vec<String>, ApiError> {
        if let Some(paths) = self.list_via_git(project_path) {
            return Ok(paths);
        }

        let mut paths = Vec::new();
        self.walk_dir(project_path, project_path, &mut paths, 0)?;
        paths.sort();
        Ok(paths)
    }

    /// Try listing files via git ls-files. Returns None if not a git repo.
    fn list_via_git(&self, project_path: &Path) -> Option<Vec<String>> {
        let output = Command::new("git")
            .args([
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "--full-name",
            ])
            .current_dir(project_path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.to_owned())
                .collect(),
        )
    }

    fn walk_dir(
        &self,
        root: &Path,
        dir: &Path,
        out: &mut Vec<String>,
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

            let ft = entry.file_type().map_err(|e| ApiError::Io(e.to_string()))?;

            if ft.is_dir() {
                if SKIP_DIRS.contains(&&*name_str) || name_str.starts_with('.') {
                    continue;
                }
                self.walk_dir(root, &entry.path(), out, depth + 1)?;
            } else if ft.is_file() || ft.is_symlink() {
                if let Ok(rel) = entry.path().strip_prefix(root) {
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
        }
        Ok(())
    }
}
