use crate::commands::git::validated_git_ref;
use crate::errors::ApiError;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Deterministic Fresh session name for a Sworm project.
/// Used by both session startup (sessions.rs) and file opening.
pub(crate) fn fresh_session_name(project_id: &str) -> String {
    format!("sworm_{}", project_id)
}

/// Send files to the Sworm-managed Fresh session.
fn fresh_open(session_name: &str, files: &[&str]) -> Result<(), ApiError> {
    let mut args = vec!["--cmd", "session", "open-file", session_name];
    args.extend(files);

    let output = Command::new("fresh")
        .args(&args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| ApiError::Io(format!("Failed to run fresh: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApiError::Io(format!("fresh: {}", stderr.trim())));
    }

    Ok(())
}

/// Open a file in the running Fresh editor session for the project.
#[tauri::command]
pub fn editor_open_file(
    project_id: String,
    project_path: String,
    file_path: String,
) -> Result<(), ApiError> {
    let abs_path = Path::new(&project_path).join(&file_path);
    fresh_open(
        &fresh_session_name(&project_id),
        &[&abs_path.to_string_lossy()],
    )
}

/// Open a file at a specific commit as a read-only snapshot in the editor.
#[tauri::command]
pub fn editor_open_at_commit(
    project_id: String,
    project_path: String,
    commit_hash: String,
    file_path: String,
) -> Result<(), ApiError> {
    validated_git_ref(&commit_hash)?;

    let repo = Path::new(&project_path);

    // Try the file at this commit, then its parent (for deletions)
    let spec1 = format!("{}:{}", commit_hash, file_path);
    let content = git_show(repo, &spec1)
        .or_else(|| git_show(repo, &format!("{}~1:{}", commit_hash, file_path)))
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "File {} not found at commit {} or its parent",
                file_path, commit_hash
            ))
        })?;

    let short_hash = &commit_hash[..7.min(commit_hash.len())];
    let temp_dir: PathBuf = std::env::temp_dir().join("sworm-viewer").join(short_hash);
    let temp_file = temp_dir.join(&file_path);

    if let Some(parent) = temp_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApiError::Io(format!("Failed to create temp directory: {}", e)))?;
    }

    // Clear read-only from a previous view so we can overwrite
    if let Ok(meta) = std::fs::metadata(&temp_file) {
        if meta.permissions().readonly() {
            let mut perms = meta.permissions();
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(&temp_file, perms);
        }
    }

    std::fs::write(&temp_file, &content)
        .map_err(|e| ApiError::Io(format!("Failed to write temp file: {}", e)))?;

    // Mark read-only to prevent accidental edits to historical snapshots
    if let Ok(meta) = std::fs::metadata(&temp_file) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(&temp_file, perms);
    }

    fresh_open(
        &fresh_session_name(&project_id),
        &[&temp_file.to_string_lossy()],
    )
}

fn git_show(repo_path: &Path, rev_spec: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["--no-optional-locks", "show", rev_spec])
        .current_dir(repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8(output.stdout).ok()
}
