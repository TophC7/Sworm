use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::provider::ProviderStatus;

#[tauri::command]
pub async fn provider_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    state.host.provider_list().await
}
