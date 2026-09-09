use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::config_schemas::ConfigSchemaEntry;

#[tauri::command]
pub async fn config_schemas_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ConfigSchemaEntry>, ApiError> {
    state.host.config_schemas_list().await
}
