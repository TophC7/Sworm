use crate::app_state::AppState;
use crate::host_events::channel_sink;
use sworm_core::errors::ApiError;
use sworm_protocol::pty::PtyEvent;
use sworm_protocol::task::TaskDefinition;

#[tauri::command]
pub async fn tasks_list(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TaskDefinition>, ApiError> {
    state.host.tasks_list(folder_path).await
}

#[tauri::command]
pub async fn tasks_start(
    run_id: String,
    folder_path: String,
    task_id: String,
    active_file_path: Option<String>,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .tasks_start(
            run_id,
            folder_path,
            task_id,
            active_file_path,
            cols,
            rows,
            channel_sink(output),
            channel_sink(events),
            Some(window.label().to_string()),
        )
        .await
}

#[tauri::command]
pub async fn tasks_write(
    run_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.tasks_write(run_id, data).await
}

#[tauri::command]
pub async fn tasks_resize(
    run_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.tasks_resize(run_id, cols, rows).await
}

#[tauri::command]
pub async fn tasks_stop(run_id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.host.tasks_stop(run_id).await
}
