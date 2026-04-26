use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::lsp::{LspEvent, LspServerSettingsEntry};
use crate::services::builtins::BuiltinCatalogService;
use crate::services::lsp::{resolve_launch, resolve_server_status, ProjectLspEnvironment};
use crate::services::nix::NixService;
use crate::services::settings::{LspServerConfigRecord, LspTraceLevel, SettingsService};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SaveLspServerConfigInput {
    pub server_definition_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub runtime_path_override: Option<String>,
    pub runtime_args: Vec<String>,
    pub extra_args: Vec<String>,
    pub trace: LspTraceLevel,
    pub settings_json: Option<String>,
}

#[tauri::command]
pub fn lsp_list_servers(
    state: tauri::State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<LspServerSettingsEntry>, ApiError> {
    let db = state.db.read();
    let env = if let Some(project_id) = project_id.as_deref() {
        let nix_env =
            NixService::load_env_vars(db.conn(), project_id).map_err(ApiError::Database)?;
        ProjectLspEnvironment::from_nix(&state.env, nix_env.as_ref())
    } else {
        ProjectLspEnvironment::from_host(&state.env)
    };

    let mut entries = Vec::new();
    for server in BuiltinCatalogService::list_server_definitions().map_err(ApiError::Internal)? {
        let server_definition_id = server.server_definition_id.clone();
        let config = SettingsService::get_lsp_server_config(db.conn(), &server_definition_id)
            .map_err(ApiError::Database)?
            .unwrap_or_else(|| LspServerConfigRecord::default_for(&server_definition_id));
        let status = resolve_server_status(&server, &config, &env);
        entries.push(LspServerSettingsEntry {
            server: status,
            config,
        });
    }

    Ok(entries)
}

#[tauri::command]
pub fn lsp_set_server_config(
    config: SaveLspServerConfigInput,
    state: tauri::State<'_, AppState>,
) -> Result<LspServerConfigRecord, ApiError> {
    let record = LspServerConfigRecord {
        server_definition_id: config.server_definition_id,
        enabled: config.enabled,
        binary_path_override: config.binary_path_override,
        runtime_path_override: config.runtime_path_override,
        runtime_args: config.runtime_args,
        extra_args: config.extra_args,
        trace: config.trace,
        settings_json: config.settings_json,
    };

    let db = state.db.write();
    SettingsService::save_lsp_server_config(db.conn(), &record).map_err(ApiError::Database)?;
    Ok(record)
}

#[tauri::command]
pub fn lsp_start(
    session_id: String,
    project_id: String,
    server_definition_id: String,
    root_path: String,
    events: tauri::ipc::Channel<LspEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let server = BuiltinCatalogService::find_server_definition(&server_definition_id)
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::NotFound(format!(
                "Unknown LSP server definition {}",
                server_definition_id
            ))
        })?;

    let db = state.db.read();
    let project = state
        .projects
        .get(db.conn(), &project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

    let root_path = normalize_root_path(&project.path, &root_path)?;
    let config = SettingsService::get_lsp_server_config(db.conn(), &server_definition_id)
        .map_err(ApiError::Database)?
        .unwrap_or_else(|| LspServerConfigRecord::default_for(&server_definition_id));

    if !config.enabled {
        return Err(ApiError::InvalidArgument(format!(
            "LSP server {} is disabled",
            server_definition_id
        )));
    }

    let nix_env = NixService::load_env_vars(db.conn(), &project_id).map_err(ApiError::Database)?;
    drop(db);

    let env = ProjectLspEnvironment::from_nix(&state.env, nix_env.as_ref());
    let resolved =
        resolve_launch(&server, &config, &env, &root_path).map_err(ApiError::Internal)?;

    state
        .lsp
        .spawn(session_id, config.trace, resolved, events)
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn lsp_send(
    session_id: String,
    message_json: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .lsp
        .send(&session_id, &message_json)
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub fn lsp_stop(session_id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.lsp.kill(&session_id).map_err(ApiError::Internal)
}

fn normalize_root_path(project_path: &str, root_path: &str) -> Result<String, ApiError> {
    let candidate = if root_path.trim().is_empty() {
        std::path::PathBuf::from(project_path)
    } else {
        std::path::PathBuf::from(root_path)
    };

    let project = std::path::Path::new(project_path);
    if !candidate.starts_with(project) {
        return Err(ApiError::InvalidArgument(format!(
            "LSP root must stay inside project {}",
            project_path
        )));
    }

    Ok(candidate.to_string_lossy().to_string())
}
