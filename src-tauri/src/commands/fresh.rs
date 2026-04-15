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

/// Write content to a read-only temp file under `sworm-viewer/<label>/` and
/// open it in the project's Fresh session.
fn write_readonly_snapshot(
    project_id: &str,
    label: &str,
    file_path: &str,
    content: &str,
) -> Result<(), ApiError> {
    let temp_dir: PathBuf = std::env::temp_dir().join("sworm-viewer").join(label);
    let temp_file = temp_dir.join(file_path);

    if let Some(parent) = temp_file.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ApiError::Io(format!("Failed to create temp directory: {}", e)))?;
    }

    std::fs::write(&temp_file, content)
        .map_err(|e| ApiError::Io(format!("Failed to write temp file: {}", e)))?;

    if let Ok(meta) = std::fs::metadata(&temp_file) {
        let mut perms = meta.permissions();
        perms.set_readonly(true);
        let _ = std::fs::set_permissions(&temp_file, perms);
    }

    fresh_open(
        &fresh_session_name(project_id),
        &[&temp_file.to_string_lossy()],
    )
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

    let spec = format!("{}:{}", commit_hash, file_path);
    let content = git_show(repo, &spec)
        .or_else(|| git_show(repo, &format!("{}~1:{}", commit_hash, file_path)))
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "File {} not found at commit {} or its parent",
                file_path, commit_hash
            ))
        })?;

    let label = &commit_hash[..7.min(commit_hash.len())];
    write_readonly_snapshot(&project_id, label, &file_path, &content)
}

/// Open a file from a stash entry as a read-only snapshot in the editor.
#[tauri::command]
pub fn editor_open_at_stash(
    project_id: String,
    project_path: String,
    stash_index: usize,
    file_path: String,
) -> Result<(), ApiError> {
    let repo = Path::new(&project_path);
    let stash_ref = format!("stash@{{{}}}", stash_index);

    let spec = format!("{}:{}", stash_ref, file_path);
    let content = git_show(repo, &spec)
        .or_else(|| {
            let untracked_spec = format!("{}^3:{}", stash_ref, file_path);
            git_show(repo, &untracked_spec)
        })
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "File {} not found in stash@{{{}}}",
                file_path, stash_index
            ))
        })?;

    let label = format!("stash-{}", stash_index);
    write_readonly_snapshot(&project_id, &label, &file_path, &content)
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
