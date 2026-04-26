use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::formatting::FormattingService;
use crate::services::nix::NixService;
use std::collections::HashMap;

#[tauri::command]
pub async fn formatting_format_biome(
    project_id: String,
    file_path: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let (project_path, env) = load_project_formatter_context(&state, &project_id)?;
    tokio::task::spawn_blocking(move || {
        FormattingService::format_with_biome(&project_path, &file_path, &content, &env)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
    .map_err(ApiError::Internal)
}

#[tauri::command]
pub async fn formatting_format_nixfmt(
    project_id: String,
    content: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let (project_path, env) = load_project_formatter_context(&state, &project_id)?;
    tokio::task::spawn_blocking(move || {
        FormattingService::format_with_nixfmt(&project_path, &content, &env)
    })
    .await
    .map_err(|error| ApiError::Internal(error.to_string()))?
    .map_err(ApiError::Internal)
}

fn load_project_formatter_context(
    state: &tauri::State<'_, AppState>,
    project_id: &str,
) -> Result<(String, HashMap<String, String>), ApiError> {
    let db = state.db.read();
    let project = state
        .projects
        .get(db.conn(), project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

    let host_env: HashMap<String, String> = std::env::vars().collect();
    let env = match NixService::load_env_vars(db.conn(), project_id).map_err(ApiError::Database)? {
        Some(nix_env) => NixService::merge_env(&host_env, &nix_env),
        None => host_env,
    };

    Ok((project.path, env))
}
