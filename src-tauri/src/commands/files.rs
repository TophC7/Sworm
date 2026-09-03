use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::files::{DirEntry, FilePasteCollision, PathList};
use std::collections::HashMap;
use std::path::Path;

/// Read the contents of a file inside a project.
#[tauri::command]
pub async fn file_read(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.files.read(Path::new(&project_path), &file_path)
}

/// Write content to a file inside a project.
#[tauri::command]
pub async fn file_write(
    project_path: String,
    file_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .files
        .write(Path::new(&project_path), &file_path, &content)
}

/// Create a directory inside a project.
#[tauri::command]
pub async fn file_create_dir(
    project_path: String,
    dir_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.files.create_dir(Path::new(&project_path), &dir_path)
}

/// Rename a file inside a project.
#[tauri::command]
pub async fn file_rename(
    project_path: String,
    old_path: String,
    new_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .files
        .rename(Path::new(&project_path), &old_path, &new_path)
}

/// Paste files into a target directory inside the project.
/// `op` is "copy" or "cut". Sources are absolute paths from the clipboard.
/// Returns the list of new project-relative paths.
#[tauri::command]
pub async fn file_paste(
    project_path: String,
    target_dir: String,
    op: String,
    sources: Vec<String>,
    collision_policy: String,
    rename_map: Option<HashMap<String, String>>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, ApiError> {
    let rename_map = rename_map.unwrap_or_default();
    state.files.paste(
        Path::new(&project_path),
        &target_dir,
        &op,
        &sources,
        &collision_policy,
        &rename_map,
    )
}

#[tauri::command]
pub async fn file_paste_collisions(
    project_path: String,
    target_dir: String,
    sources: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FilePasteCollision>, ApiError> {
    state
        .files
        .paste_collisions(Path::new(&project_path), &target_dir, &sources)
}

/// Delete a file or directory inside a project.
#[tauri::command]
pub async fn file_delete(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.files.delete(Path::new(&project_path), &file_path)
}

/// List one directory as the explorer renders it. `dir_path` is
/// project-relative; "" is the project root.
#[tauri::command]
pub async fn files_read_dir(
    project_path: String,
    dir_path: String,
    show_hidden: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DirEntry>, ApiError> {
    let generation = *state.settings_generation.lock();
    state
        .files
        .read_dir(Path::new(&project_path), &dir_path, show_hidden, generation)
}

/// Flat list of searchable file paths, for Quick Open and the sidebar filter.
/// The walk visits the whole project, so it runs on a blocking worker rather
/// than on a Tauri runtime thread.
#[tauri::command]
pub async fn files_list_paths(
    project_path: String,
    show_hidden: bool,
    state: tauri::State<'_, AppState>,
) -> Result<PathList, ApiError> {
    let generation = *state.settings_generation.lock();
    let files = std::sync::Arc::clone(&state.files);
    tokio::task::spawn_blocking(move || {
        files.list_paths(Path::new(&project_path), show_hidden, generation)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
}

/// Watch exactly the directories the explorer currently renders. A watcher
/// failure costs freshness, never the listing itself, so it is logged rather
/// than surfaced.
#[tauri::command]
pub async fn files_watch_dirs(
    app: tauri::AppHandle,
    project_path: String,
    dirs: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    if let Err(error) = state
        .file_watchers
        .sync(&app, Path::new(&project_path), &dirs)
    {
        tracing::warn!(folder = %project_path, %error, "explorer watcher unavailable");
    }
    Ok(())
}
