use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::{
    nix_env::{NixDetection, NixDiagnostic, NixEnvRecord},
    provider::ProviderStatus,
};

#[tauri::command]
pub async fn nix_detect(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixDetection, ApiError> {
    state.router.nix_detect(folder_path).await
}

#[tauri::command]
pub async fn nix_select(
    folder_path: String,
    nix_file: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixEnvRecord, ApiError> {
    state.router.nix_select(folder_path, nix_file).await
}

#[tauri::command]
pub async fn nix_evaluate(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixEnvRecord, ApiError> {
    state.router.nix_evaluate(folder_path).await
}

#[tauri::command]
pub async fn nix_clear(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.router.nix_clear(folder_path).await
}

#[tauri::command]
pub async fn nix_lint(
    folder_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<NixDiagnostic>, ApiError> {
    state.router.nix_lint(folder_path, file_path).await
}

#[tauri::command]
pub async fn provider_list_for_folder(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    state.router.provider_list_for_folder(folder_path).await
}
