use crate::app_state::AppState;
use sworm_core::errors::ApiError;

#[tauri::command]
pub async fn formatting_format_biome(
    folder_path: String,
    file_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state
        .host
        .formatting_format_biome(folder_path, file_path, content)
        .await
}

#[tauri::command]
pub async fn formatting_format_nixfmt(
    folder_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state
        .host
        .formatting_format_nixfmt(folder_path, content)
        .await
}
