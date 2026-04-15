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

/// List all files in the project.
#[tauri::command]
pub fn files_list_all(
    project_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, ApiError> {
    state.files.list_all(Path::new(&project_path))
}
