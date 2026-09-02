use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::folders::resolve_folder;
use crate::services::formatting::FormattingService;
use crate::services::nix::NixService;
use std::collections::HashMap;

#[tauri::command]
pub async fn formatting_format_biome(
    folder_path: String,
    file_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let (folder_path, env) = formatter_context(&state, &folder_path)?;
    tokio::task::spawn_blocking(move || {
        FormattingService::format_with_biome(&folder_path, &file_path, &content, &env)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
    .map_err(ApiError::Internal)
}

#[tauri::command]
pub async fn formatting_format_nixfmt(
    folder_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let (folder_path, env) = formatter_context(&state, &folder_path)?;
    tokio::task::spawn_blocking(move || {
        FormattingService::format_with_nixfmt(&folder_path, &content, &env)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
    .map_err(ApiError::Internal)
}

/// Canonical folder path plus host env overlaid with the folder's Nix env.
fn formatter_context(
    state: &tauri::State<'_, AppState>,
    folder_path: &str,
) -> Result<(String, HashMap<String, String>), ApiError> {
    let folder_path = resolve_folder(folder_path)?.to_string_lossy().into_owned();
    let db = state.db.read();
    let host_env: HashMap<String, String> = std::env::vars().collect();
    let env =
        match NixService::load_env_vars(db.conn(), &folder_path).map_err(ApiError::Database)? {
            Some(nix_env) => NixService::merge_env(&host_env, &nix_env),
            None => host_env,
        };
    Ok((folder_path, env))
}
