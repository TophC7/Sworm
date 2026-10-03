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
