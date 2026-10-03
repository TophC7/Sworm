use crate::app_state::AppState;
use sworm_core::errors::ApiError;

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

#[tauri::command]
pub async fn file_read_stream(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
    project_path: String,
    file_path: String,
    version: String,
    size: u64,
) -> Result<sworm_protocol::files::FileContent, ApiError> {
    use tauri::Emitter;
    state.router.read_file_stream(window.label(), &request_id, project_path.clone(), file_path.clone(), version, size, |bytes, total| {
        let _ = window.emit("file-read-progress", serde_json::json!({
            "requestId": request_id, "folderPath": project_path, "filePath": file_path, "bytes": bytes, "total": total,
        }));
    }).await
}

#[tauri::command]
pub fn file_read_stream_cancel(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    request_id: String,
) {
    state.router.cancel_file_read(window.label(), &request_id);
}
