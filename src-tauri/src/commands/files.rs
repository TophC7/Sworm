use crate::app_state::AppState;
use crate::errors::ApiError;
use std::path::Path;

/// Read the contents of a file inside a project.
#[tauri::command]
pub fn file_read(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.files.read(Path::new(&project_path), &file_path)
}

/// Write content to a file inside a project.
#[tauri::command]
pub fn file_write(
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
pub fn file_create_dir(
    project_path: String,
    dir_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.files.create_dir(Path::new(&project_path), &dir_path)
}

/// Rename a file inside a project.
#[tauri::command]
pub fn file_rename(
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
pub fn file_paste(
    project_path: String,
    target_dir: String,
    op: String,
    sources: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, ApiError> {
    state
        .files
        .paste(Path::new(&project_path), &target_dir, &op, &sources)
}

/// Delete a file or directory inside a project.
#[tauri::command]
pub fn file_delete(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.files.delete(Path::new(&project_path), &file_path)
}

/// List all files in the project.
#[tauri::command]
pub fn files_list_all(
    project_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, ApiError> {
    state.files.list_all(Path::new(&project_path))
}
