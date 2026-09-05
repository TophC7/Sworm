use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::files::{DirEntry, FilePasteCollision, FilePasteMapping, PathList};
use crate::services::folders::{normalize_absolute_path, resolve_folder};
use std::collections::HashMap;
use std::path::Path;
use tauri::Emitter;

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
    app: tauri::AppHandle,
    project_path: String,
    old_path: String,
    new_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project = resolve_folder(&project_path)?;
    state.files.validate_path(&old_path)?;
    state.files.validate_path(&new_path)?;
    let old_abs = normalize_absolute_path(&project.join(&old_path));
    state.files.rename(&project, &old_path, &new_path)?;
    let new_abs = normalize_absolute_path(&project.join(&new_path));
    state.windows.rename_claims_under(&old_abs, &new_abs);
    app.emit(
        "sworm://file-path-changed",
        serde_json::json!({
            "oldPath": old_abs.to_string_lossy(),
            "newPath": new_abs.to_string_lossy(),
            "folderPath": project.to_string_lossy(),
        }),
    )
    .map_err(|error| ApiError::Internal(error.to_string()))
}

/// Paste files into a target directory inside the project.
/// `op` is "copy" or "cut". Sources are absolute paths from the clipboard.
/// Returns each transferred source and its new project-relative path.
#[tauri::command]
pub async fn file_paste(
    app: tauri::AppHandle,
    project_path: String,
    target_dir: String,
    op: String,
    sources: Vec<String>,
    collision_policy: String,
    rename_map: Option<HashMap<String, String>>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FilePasteMapping>, ApiError> {
    let project = resolve_folder(&project_path)?;
    let source_paths = if op == "cut" {
        sources
            .iter()
            .map(|source| {
                Ok((
                    source.clone(),
                    normalize_absolute_path(&std::path::absolute(source)?),
                ))
            })
            .collect::<Result<HashMap<_, _>, ApiError>>()?
    } else {
        HashMap::new()
    };
    let rename_map = rename_map.unwrap_or_default();
    let mappings = state.files.paste(
        &project,
        &target_dir,
        &op,
        &sources,
        &collision_policy,
        &rename_map,
    )?;
    if op == "cut" {
        for mapping in &mappings {
            let source_abs = source_paths
                .get(&mapping.source)
                .expect("cut source normalized before paste");
            let new_abs = normalize_absolute_path(&project.join(&mapping.destination));
            let displaced = state.windows.release_claims_under(&new_abs);
            if displaced > 0 {
                let _ = app.emit(
                    "sworm://file-deleted",
                    serde_json::json!({ "filePath": new_abs.to_string_lossy() }),
                );
            }
            state.windows.rename_claims_under(source_abs, &new_abs);
            app.emit(
                "sworm://file-path-changed",
                serde_json::json!({
                    "oldPath": source_abs.to_string_lossy(),
                    "newPath": new_abs.to_string_lossy(),
                    "folderPath": project.to_string_lossy(),
                }),
            )
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        }
    }
    Ok(mappings)
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
    app: tauri::AppHandle,
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project = resolve_folder(&project_path)?;
    state.files.validate_path(&file_path)?;
    let abs = normalize_absolute_path(&project.join(&file_path));
    state.files.delete(&project, &file_path)?;
    state.windows.release_claims_under(&abs);
    app.emit(
        "sworm://file-deleted",
        serde_json::json!({ "filePath": abs.to_string_lossy() }),
    )
    .map_err(|error| ApiError::Internal(error.to_string()))
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
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    project_path: String,
    dirs: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    if let Err(error) =
        state
            .file_watchers
            .sync(&app, window.label(), Path::new(&project_path), &dirs)
    {
        tracing::warn!(folder = %project_path, %error, "explorer watcher unavailable");
    }
    Ok(())
}
