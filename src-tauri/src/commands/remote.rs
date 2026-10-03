use crate::{app_state::AppState, router::RemoteStatus};
use sworm_core::errors::ApiError;
use sworm_protocol::settings::RemoteSettings;

#[tauri::command]
pub async fn pair_remote(
    state: tauri::State<'_, AppState>,
    link: String,
    name: String,
) -> Result<RemoteSettings, ApiError> {
    state.router.pair_remote(&link, &name, false).await
}

#[tauri::command]
pub async fn repair_remote(
    state: tauri::State<'_, AppState>,
    link: String,
    name: String,
) -> Result<RemoteSettings, ApiError> {
    state.router.pair_remote(&link, &name, true).await
}

/// This desktop's identity fingerprint, which servers list in `authorized_keys`.
#[tauri::command]
pub async fn remote_client_fingerprint(
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.router.client_fingerprint().await
}

#[tauri::command]
pub async fn remote_status(
    state: tauri::State<'_, AppState>,
    server: String,
) -> Result<RemoteStatus, ApiError> {
    state.router.remote_status(&server).await
}

#[tauri::command]
pub fn remote_runs_release(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    run_ids: Vec<String>,
) -> Result<(), ApiError> {
    state.router.remote_runs_release(window.label(), &run_ids)
}

#[tauri::command]
pub async fn rename_remote(
    state: tauri::State<'_, AppState>,
    server: String,
    name: String,
) -> Result<(), ApiError> {
    if state.windows.remote_claimed(&server) {
        return Err(ApiError::InvalidArgument(
            "Close all tabs for this remote before renaming or removing it".into(),
        ));
    }
    state.router.change_remote(&server, Some(&name)).await
}

#[tauri::command]
pub async fn remove_remote(
    state: tauri::State<'_, AppState>,
    server: String,
) -> Result<(), ApiError> {
    if state.windows.remote_claimed(&server) {
        return Err(ApiError::InvalidArgument(
            "Close all tabs for this remote before renaming or removing it".into(),
        ));
    }
    state.router.change_remote(&server, None).await
}
