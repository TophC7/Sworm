use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::lsp::{LspEvent, LspServerSettingsEntry};
use crate::models::settings::{LspServerConfigRecord, LspTraceLevel};
use crate::services::builtins::BuiltinCatalogService;
use crate::services::folders::resolve_folder;
use crate::services::lsp::{resolve_launch, resolve_server_status, ProjectLspEnvironment};
use crate::services::nix::NixService;
use crate::services::settings::SettingsService;
use crate::services::settings_patch::patch_top_level_section;
use crate::services::settings_resolution::{
    lsp_config_record, resolve_effective_settings_for_folder_path,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct SaveLspServerConfigInput {
    pub server_definition_id: String,
    pub enabled: bool,
    pub binary_path_override: Option<String>,
    pub runtime_path_override: Option<String>,
    pub runtime_args: Vec<String>,
    pub extra_args: Vec<String>,
    pub trace: LspTraceLevel,
    pub settings: Option<Value>,
}

#[tauri::command]
pub async fn lsp_list_servers(
    state: tauri::State<'_, AppState>,
    folder_path: Option<String>,
) -> Result<Vec<LspServerSettingsEntry>, ApiError> {
    let (project_path, env) = if let Some(folder_path) = folder_path.as_deref() {
        let folder = resolve_folder(folder_path)?;
        let db = state.db.read();
        let nix_env = NixService::load_env_vars(db.conn(), &folder.to_string_lossy())
            .map_err(ApiError::Database)?;
        (
            Some(folder),
            ProjectLspEnvironment::from_nix(&state.env, nix_env.as_ref()),
        )
    } else {
        (None, ProjectLspEnvironment::from_host(&state.env))
    };

    let effective = resolve_effective_settings_for_folder_path(project_path.as_deref())
        .map_err(ApiError::Internal)?;

    let mut entries = Vec::new();
    for server in BuiltinCatalogService::list_server_definitions().map_err(ApiError::Internal)? {
        let server_definition_id = server.server_definition_id.clone();
        let config = lsp_config_record(&effective.settings, &server_definition_id);
        let status = resolve_server_status(&server, &config, &env);
        entries.push(LspServerSettingsEntry {
            server: status,
            config,
        });
    }

    Ok(entries)
}

#[tauri::command]
pub async fn lsp_set_server_config(
    config: SaveLspServerConfigInput,
) -> Result<LspServerConfigRecord, ApiError> {
    let record = LspServerConfigRecord {
        server_definition_id: config.server_definition_id,
        enabled: config.enabled,
        binary_path_override: config.binary_path_override,
        runtime_path_override: config.runtime_path_override,
        runtime_args: config.runtime_args,
        extra_args: config.extra_args,
        trace: config.trace,
        settings: config.settings,
    };

    patch_global_lsp_server(&record)?;
    Ok(record)
}

#[tauri::command]
pub async fn lsp_start(
    window: tauri::WebviewWindow,
    session_id: String,
    folder_path: String,
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

    let folder = resolve_folder(&folder_path)?;
    let folder_path = folder.to_string_lossy().into_owned();

    let root_path = normalize_root_path(&folder_path, &root_path)?;
    let effective =
        resolve_effective_settings_for_folder_path(Some(&folder)).map_err(ApiError::Internal)?;
    let config = lsp_config_record(&effective.settings, &server_definition_id);

    if !config.enabled {
        return Err(ApiError::InvalidArgument(format!(
            "LSP server {} is disabled",
            server_definition_id
        )));
    }

    let db = state.db.read();
    let nix_env = NixService::load_env_vars(db.conn(), &folder_path).map_err(ApiError::Database)?;
    drop(db);

    let env = ProjectLspEnvironment::from_nix(&state.env, nix_env.as_ref());
    let resolved =
        resolve_launch(&server, &config, &env, &root_path).map_err(ApiError::Internal)?;

    state
        .lsp
        .spawn(
            session_id,
            Some(window.label().to_string()),
            config.trace,
            resolved,
            events,
        )
        .map_err(ApiError::Internal)
}

#[tauri::command]
pub async fn lsp_send(
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
pub async fn lsp_stop(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.lsp.kill(&session_id).map_err(ApiError::Internal)
}

fn patch_global_lsp_server(record: &LspServerConfigRecord) -> Result<(), ApiError> {
    let path = ensure_global_settings_file()?;
    patch_lsp_server_file(&path, record)
}

fn patch_lsp_server_file(path: &PathBuf, record: &LspServerConfigRecord) -> Result<(), ApiError> {
    let original = std::fs::read_to_string(path).map_err(|error| {
        ApiError::Io(format!(
            "Failed to read settings file {}: {}",
            path.display(),
            error
        ))
    })?;
    let mut root = if original.trim().is_empty() {
        json!({})
    } else {
        jsonc_parser::parse_to_serde_value::<Value>(&original, &Default::default())
            .map_err(|error| ApiError::Internal(format!("Invalid settings JSONC: {error}")))?
    };
    let lsp = ensure_object_property(&mut root, "lsp")?;
    let servers = ensure_nested_object_property(lsp, "servers")?;
    servers.insert(
        record.server_definition_id.clone(),
        json!({
            "enabled": record.enabled,
            "binary_path_override": record.binary_path_override,
            "runtime_path_override": record.runtime_path_override,
            "runtime_args": record.runtime_args,
            "extra_args": record.extra_args,
            "trace": record.trace,
            "settings": record.settings,
        }),
    );

    let patched =
        patch_top_level_section(&original, "lsp", &root["lsp"]).map_err(ApiError::Internal)?;
    std::fs::write(path, patched).map_err(|error| {
        ApiError::Io(format!(
            "Failed to write settings file {}: {}",
            path.display(),
            error
        ))
    })
}

fn ensure_global_settings_file() -> Result<PathBuf, ApiError> {
    let path = SettingsService::ensure_global_settings_parent().map_err(ApiError::Internal)?;
    if !path.exists() {
        std::fs::write(&path, "{\n}\n").map_err(|error| {
            ApiError::Io(format!(
                "Failed to create settings file {}: {}",
                path.display(),
                error
            ))
        })?;
    }
    Ok(path)
}

fn ensure_object_property<'a>(
    root: &'a mut Value,
    key: &str,
) -> Result<&'a mut Map<String, Value>, ApiError> {
    if root.is_null() {
        *root = json!({});
    }
    let object = root
        .as_object_mut()
        .ok_or_else(|| ApiError::InvalidArgument("Settings root must be an object".to_string()))?;
    ensure_nested_object_property(object, key)
}

fn ensure_nested_object_property<'a>(
    object: &'a mut Map<String, Value>,
    key: &str,
) -> Result<&'a mut Map<String, Value>, ApiError> {
    let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
    if value.is_null() {
        *value = json!({});
    }
    value.as_object_mut().ok_or_else(|| {
        ApiError::InvalidArgument(format!("Settings `{key}` section must be an object"))
    })
}

fn normalize_root_path(project_path: &str, root_path: &str) -> Result<String, ApiError> {
    let candidate = if root_path.trim().is_empty() {
        PathBuf::from(project_path)
    } else {
        PathBuf::from(root_path)
    };

    let project = Path::new(project_path);
    if !candidate.starts_with(project) {
        return Err(ApiError::InvalidArgument(format!(
            "LSP root must stay inside project {}",
            project_path
        )));
    }

    Ok(candidate.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sworm-lsp-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn patch_lsp_server_file_preserves_native_settings_object() {
        let root = temp_root("patch");
        std::fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        std::fs::write(
            &path,
            r#"{
  "future_top": true,
  "lsp": { "servers": {} }
}
"#,
        )
        .expect("settings file written");

        let record = LspServerConfigRecord {
            server_definition_id: "dev.sworm.vtsls::vtsls".to_string(),
            enabled: true,
            binary_path_override: None,
            runtime_path_override: None,
            runtime_args: vec![],
            extra_args: vec!["--stdio".to_string()],
            trace: LspTraceLevel::Messages,
            settings: Some(json!({ "typescript": { "preferences": { "quoteStyle": "single" } } })),
        };

        patch_lsp_server_file(&path, &record).expect("patched");
        let patched = std::fs::read_to_string(&path).expect("patched read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(
            parsed["lsp"]["servers"]["dev.sworm.vtsls::vtsls"]["settings"]["typescript"]
                ["preferences"]["quoteStyle"],
            json!("single")
        );
        std::fs::remove_dir_all(root).expect("temp dir removed");
    }
}
