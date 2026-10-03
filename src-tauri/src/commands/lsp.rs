use crate::app_state::AppState;
use crate::host_events::channel_sink;
use sworm_core::errors::ApiError;
use sworm_protocol::lsp::LspEvent;

#[tauri::command]
pub async fn lsp_start(
    window: tauri::WebviewWindow,
    session_id: String,
    folder_path: String,
    server_definition_id: String,
    root_path: String,
    events: tauri::ipc::Channel<LspEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .router
        .lsp_start(
            Some(window.label().to_string()),
            session_id,
            folder_path,
            server_definition_id,
            root_path,
            channel_sink(events),
        )
        .await
}

#[tauri::command]
pub async fn lsp_send(
    session_id: String,
    message_json: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.router.lsp_send(session_id, message_json).await
}
