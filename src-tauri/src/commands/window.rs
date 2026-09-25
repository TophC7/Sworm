use crate::app_state::AppState;
use crate::host_events::channel_sink;
use crate::router::Target;
use crate::services::windows::{
    ClaimFileResult, TabTransferExportPayload, TabTransferInitiateParams,
};
use std::path::{Path, PathBuf};
use sworm_core::{errors::ApiError, services::pty::PtySubscriber};
use sworm_protocol::pty::PtyEvent;
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
pub async fn window_ready(
    window: WebviewWindow,
    restored_task_runs: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let host = state.host.clone();
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || {
        host.release_unrestored_tasks(&owner, &restored_task_runs.into_iter().collect())
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))??;
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
pub async fn pty_pause(
    window: WebviewWindow,
    run_id: String,
    state: State<'_, AppState>,
) -> Result<u64, ApiError> {
    let host = state.host.clone();
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || host.pty.pause_owned(&run_id, &owner))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Pty)
}

#[tauri::command]
pub async fn pty_attach(
    window: WebviewWindow,
    transfer_id: String,
    run_id: String,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: State<'_, AppState>,
) -> Result<u64, ApiError> {
    let host = state.host.clone();
    let windows = state.windows.clone();
    let owner = window.label().to_owned();
    let subscriber = PtySubscriber {
        output: channel_sink(output),
        events: channel_sink(events),
    };
    tokio::task::spawn_blocking(move || {
        windows.attach_transfer_pty(&transfer_id, &owner, &run_id, subscriber, &host.pty)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
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
pub async fn window_transfer_initiate(
    app: AppHandle,
    window: WebviewWindow,
    params: TabTransferInitiateParams,
    state: State<'_, AppState>,
) -> Result<String, ApiError> {
    authorize_transfer_initiation(window.label(), &params)?;
    let windows = state.windows.clone();
    tokio::task::spawn_blocking(move || windows.initiate_tab_transfer(&app, params))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Internal)
}

fn authorize_transfer_initiation(
    caller: &str,
    params: &TabTransferInitiateParams,
) -> Result<(), ApiError> {
    if params.target_window != caller {
        return Err(ApiError::Pty(
            "transfer target belongs to a different window".to_string(),
        ));
    }
    Ok(())
}

#[tauri::command]
pub async fn window_transfer_source_exported(
    app: AppHandle,
    window: WebviewWindow,
    payload: TabTransferExportPayload,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let windows = state.windows.clone();
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || windows.source_export_ready(&app, &owner, payload))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub async fn window_transfer_target_staged(
    app: AppHandle,
    window: WebviewWindow,
    transfer_id: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let windows = state.windows.clone();
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || windows.target_stage_ready(&app, &owner, &transfer_id))
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub async fn window_transfer_abort(
    app: AppHandle,
    window: WebviewWindow,
    transfer_id: String,
    reason: String,
    state: State<'_, AppState>,
) -> Result<(), ApiError> {
    let windows = state.windows.clone();
    let owner = window.label().to_owned();
    tokio::task::spawn_blocking(move || {
        windows
            .authorize_participant(&transfer_id, &owner)
            .map_err(ApiError::Pty)?;
        windows.abort_tab_transfer(&app, &transfer_id, &reason);
        Ok(())
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
}

#[tauri::command]
pub fn window_close(window: WebviewWindow) -> Result<(), ApiError> {
    window
        .close()
        .map_err(|error| ApiError::Internal(error.to_string()))
}

/// Claims are string keys, and a remote file has no local path to
/// canonicalize, so its `sworm://` URI is the key.
fn resolve_file_path(file_path: &str) -> Result<PathBuf, ApiError> {
    if let Target::Remote { .. } = Target::parse(file_path)? {
        return Ok(PathBuf::from(file_path));
    }
    let path = std::path::absolute(Path::new(file_path))?;
    Ok(sworm_core::services::folders::normalize_absolute_path(
        &path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_drop_target_can_initiate_transfer() {
        let params = TabTransferInitiateParams {
            source_window: "source".into(),
            target_window: "target".into(),
            tab_id: "tab".into(),
            target_index: 0,
        };
        assert!(authorize_transfer_initiation("target", &params).is_ok());
        assert!(authorize_transfer_initiation("source", &params).is_err());
        assert!(authorize_transfer_initiation("unrelated", &params).is_err());
    }
}
