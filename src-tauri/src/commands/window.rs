use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::pty::PtyEvent;
use crate::services::windows::{
    ClaimFileResult, TabTransferExportPayload, TabTransferInitiateParams,
};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State, WebviewWindow};

#[tauri::command]
pub fn window_create(app: AppHandle, state: State<'_, AppState>) -> Result<String, ApiError> {
    state
        .windows
        .create_workbench_window(&app, None)
        .map(|window| window.label().to_string())
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn window_ready(window: WebviewWindow, state: State<'_, AppState>) -> Result<(), ApiError> {
    state
        .windows
        .mark_ready(window.label(), window.app_handle())
        .map(|_| ())
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn window_get_label(window: WebviewWindow) -> Result<String, ApiError> {
    Ok(window.label().to_string())
}

#[tauri::command]
pub fn pty_pause(run_id: String, state: State<'_, AppState>) -> Result<u64, ApiError> {
    state.pty.pause(&run_id).map_err(ApiError::Pty)
}

#[tauri::command]
pub fn pty_attach(
    window: WebviewWindow,
    transfer_id: String,
    run_id: String,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: State<'_, AppState>,
) -> Result<u64, ApiError> {
    state
        .windows
        .authorize_attach(&transfer_id, window.label(), &run_id)
        .map_err(ApiError::Pty)?;
    state
        .pty
        .attach(&run_id, output, events)
        .map_err(ApiError::Pty)
}

#[tauri::command]
pub fn window_claim_file(
    window: WebviewWindow,
    file_path: String,
    tab_id: String,
    reveal: Option<serde_json::Value>,
    state: State<'_, AppState>,
) -> Result<ClaimFileResult, ApiError> {
    let file = resolve_file_path(&file_path)?;
    state
        .windows
        .claim_file(&window, file, tab_id, reveal)
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn window_release_file(
    window: WebviewWindow,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let file = resolve_file_path(&file_path)?;
    state.windows.release_file(window.label(), &file);
    Ok(())
}
#[tauri::command]
pub fn window_transfer_initiate(
    app: AppHandle,
    params: TabTransferInitiateParams,
    state: State<'_, AppState>,
) -> Result<String, ApiError> {
    state
        .windows
        .initiate_tab_transfer(&app, params)
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn window_transfer_source_exported(
    app: AppHandle,
    payload: TabTransferExportPayload,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .windows
        .source_export_ready(&app, payload)
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn window_transfer_target_staged(
    app: AppHandle,
    transfer_id: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .windows
        .target_stage_ready(&app, &transfer_id)
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn window_transfer_abort(
    app: AppHandle,
    transfer_id: String,
    reason: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .windows
        .abort_tab_transfer(&app, &transfer_id, &reason);
    Ok(())
}

#[tauri::command]
pub fn window_close(window: WebviewWindow) -> Result<(), ApiError> {
    window
        .close()
        .map_err(|error| ApiError::Internal(error.to_string()))
}

fn resolve_file_path(file_path: &str) -> Result<PathBuf, ApiError> {
    let path = std::path::absolute(Path::new(file_path))?;
    Ok(crate::services::folders::normalize_absolute_path(&path))
}
