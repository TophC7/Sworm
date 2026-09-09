use crate::app_state::AppState;
use crate::host_events::channel_sink;
use sworm_core::errors::ApiError;
use sworm_protocol::lsp::{LspEvent, LspServerSettingsEntry, SaveLspServerConfigInput};
use sworm_protocol::settings::LspServerConfigRecord;

#[tauri::command]
pub async fn lsp_list_servers(
    state: tauri::State<'_, AppState>,
    folder_path: Option<String>,
) -> Result<Vec<LspServerSettingsEntry>, ApiError> {
    state.host.lsp_list_servers(folder_path).await
}

#[tauri::command]
pub async fn lsp_set_server_config(
    config: SaveLspServerConfigInput,
    state: tauri::State<'_, AppState>,
) -> Result<LspServerConfigRecord, ApiError> {
    state.host.lsp_set_server_config(config).await
}

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
        .host
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
    state.host.lsp_send(session_id, message_json).await
}

#[tauri::command]
pub async fn lsp_stop(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.lsp_stop(session_id).await
}
