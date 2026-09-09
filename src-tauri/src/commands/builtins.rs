use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::builtins::BuiltinCatalog;

#[tauri::command]
pub async fn builtins_get_catalog(
    state: tauri::State<'_, AppState>,
) -> Result<BuiltinCatalog, ApiError> {
    state.host.builtins_get_catalog().await
}
