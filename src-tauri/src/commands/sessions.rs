use crate::app_state::AppState;
use crate::host_events::channel_sink;
use sworm_core::errors::ApiError;
use sworm_protocol::pty::PtyEvent;
use sworm_protocol::session::SessionStartInfo;

#[tauri::command]
pub async fn session_start(
    run_id: String,
    folder_path: String,
    provider_id: String,
    resume_token: Option<String>,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<SessionStartInfo, ApiError> {
    state
        .host
        .session_start(
            run_id,
            folder_path,
            provider_id,
            resume_token,
            cols,
            rows,
            channel_sink(output),
            channel_sink(events),
            Some(window.label().to_string()),
        )
        .await
}

#[tauri::command]
pub async fn session_write(
    run_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.session_write(run_id, data).await
}

#[tauri::command]
pub async fn session_resize(
    run_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.session_resize(run_id, cols, rows).await
}

#[tauri::command]
pub async fn session_stop(
    run_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.session_stop(run_id).await
}

#[tauri::command]
pub async fn omp_resolve_uri(
    uri: String,
    cwd: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<sworm_protocol::omp::OmpResolvedTarget, ApiError> {
    state.host.omp_resolve_uri(uri, cwd).await
}
