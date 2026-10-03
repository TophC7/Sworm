use crate::{app_state::AppState, windows::focus_window};
use sworm_core::errors::ApiError;
use sworm_protocol::rpc::{AttachMode, WorkbenchAttached, WorkbenchInfo};
use tauri::Manager;

#[tauri::command]
pub async fn workbench_list(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    server: String,
) -> Result<Vec<WorkbenchInfo>, ApiError> {
    state
        .router
        .workbench_list_for_owner(window.label(), &server)
        .await
}

#[tauri::command]
pub async fn workbench_close(
    state: tauri::State<'_, AppState>,
    server: String,
    id: String,
) -> Result<(), ApiError> {
    state.router.workbench_close(&server, id).await
}

#[tauri::command]
pub async fn workbench_attach(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    server: String,
    id: String,
    mode: AttachMode,
    attachment_id: String,
) -> Result<Option<WorkbenchAttached>, ApiError> {
    let attached = state
        .router
        .workbench_attach(
            window.label(),
            &server,
            id.clone(),
            mode,
            attachment_id.clone(),
        )
        .await?;
    if attached.is_none() {
        if let Some(owner) = state.router.workbench_owner(&server, &id, window.label()) {
            focus_window(window.app_handle(), &owner);
        }
    }
    // The RPC may finish after the window's teardown already drained its leases.
    if matches!(attached, Some(WorkbenchAttached::Ready { .. }))
        && !state.windows.has_window(window.label())
    {
        if let Err(error) = state
            .router
            .workbench_detach(window.label(), &server, id, attachment_id)
            .await
        {
            tracing::warn!(%server, %error, "late workbench attach detach failed");
        }
    }
    Ok(attached)
}

#[tauri::command]
pub async fn workbench_detach(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    server: String,
    id: String,
    attachment_id: String,
) -> Result<(), ApiError> {
    state
        .router
        .workbench_detach(window.label(), &server, id, attachment_id)
        .await
}

#[tauri::command]
pub async fn workbench_transfer(
    state: tauri::State<'_, AppState>,
    window: tauri::WebviewWindow,
    target_owner: String,
    server: String,
    id: String,
    attachment_id: String,
) -> Result<WorkbenchAttached, ApiError> {
    if !state.windows.has_window(&target_owner) {
        return Err(ApiError::NotFound(format!(
            "Unknown target window `{target_owner}`"
        )));
    }
    state
        .router
        .workbench_transfer(window.label(), &target_owner, &server, id, attachment_id)
        .await
}

#[tauri::command]
pub async fn workbench_save(
    state: tauri::State<'_, AppState>,
    server: String,
    id: String,
    snapshot: String,
) -> Result<(), ApiError> {
    state.router.workbench_save(&server, id, snapshot).await
}
