use crate::app_state::AppState;
use std::collections::HashMap;
use sworm_core::errors::ApiError;
use sworm_protocol::files::{DirEntry, FilePasteCollision, FilePasteMapping, PathList};

#[tauri::command]
pub async fn file_read(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.router.file_read(project_path, file_path).await
}

#[tauri::command]
pub async fn file_write(
    project_path: String,
    file_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .file_write(project_path, file_path, content)
        .await
}

#[tauri::command]
pub async fn file_create_dir(
    project_path: String,
    dir_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.file_create_dir(project_path, dir_path).await
}

#[tauri::command]
pub async fn file_rename(
    project_path: String,
    old_path: String,
    new_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .file_rename(project_path, old_path, new_path)
        .await
}

#[tauri::command]
pub async fn file_paste(
    project_path: String,
    target_dir: String,
    op: String,
    sources: Vec<String>,
    collision_policy: String,
    rename_map: Option<HashMap<String, String>>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FilePasteMapping>, ApiError> {
    state
        .host
        .file_paste(
            project_path,
            target_dir,
            op,
            sources,
            collision_policy,
            rename_map,
        )
        .await
}

#[tauri::command]
pub async fn file_paste_collisions(
    project_path: String,
    target_dir: String,
    sources: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FilePasteCollision>, ApiError> {
    state
        .host
        .file_paste_collisions(project_path, target_dir, sources)
        .await
}

#[tauri::command]
pub async fn file_delete(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.file_delete(project_path, file_path).await
}

#[tauri::command]
pub async fn files_read_dir(
    project_path: String,
    dir_path: String,
    show_hidden: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<DirEntry>, ApiError> {
    state
        .router
        .files_read_dir(project_path, dir_path, show_hidden)
        .await
}

#[tauri::command]
pub async fn files_list_paths(
    project_path: String,
    show_hidden: bool,
    state: tauri::State<'_, AppState>,
) -> Result<PathList, ApiError> {
    state.host.files_list_paths(project_path, show_hidden).await
}

#[tauri::command]
pub async fn files_watch_dirs(
    window: tauri::WebviewWindow,
    project_path: String,
    dirs: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .router
        .files_watch_dirs(window.label().to_string(), project_path, dirs)
        .await
}
