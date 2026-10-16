use crate::app_state::AppState;
use sworm_core::errors::ApiError;

#[tauri::command]
pub async fn git_watch(
    project_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let windows = std::sync::Arc::clone(&state.windows);
    state
        .router
        .git_watch(project_path, move |folder| windows.folder_claimed(folder))
        .await
}

#[tauri::command]
pub async fn git_worktree(
    window: tauri::WebviewWindow,
    path: String,
    op: sworm_protocol::git::GitWorktreeOp,
    state: tauri::State<'_, AppState>,
) -> Result<sworm_protocol::git::GitWorktreeResult, ApiError> {
    state
        .router
        .git_worktree(
            window.label().to_owned(),
            std::sync::Arc::clone(&state.windows),
            path,
            op,
        )
        .await
}
