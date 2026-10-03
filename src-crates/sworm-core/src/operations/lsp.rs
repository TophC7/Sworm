use crate::errors::ApiError;
use crate::events::EventSink;
use crate::host::Host;
use crate::services::builtins::BuiltinCatalogService;
use crate::services::folders::resolve_folder;
use crate::services::lsp::{resolve_launch, resolve_server_status};
use crate::services::settings::SettingsService;
use crate::services::settings_resolution::{
    lsp_config_record, resolve_effective_settings_for_folder_path,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use sworm_protocol::lsp::{LspEvent, LspServerSettingsEntry};
use sworm_protocol::settings::{LspServerConfigRecord, LspServerSettings};

impl Host {
    pub fn lsp_list_servers(
        &self,
        folder_path: Option<String>,
    ) -> Result<Vec<LspServerSettingsEntry>, ApiError> {
        let (project_path, env) = if let Some(folder_path) = folder_path.as_deref() {
            let folder = resolve_folder(folder_path)?;
            let env = self.folder_env(&folder);
            (Some(folder), env)
        } else {
            (None, self.env.with_nix(None))
        };

        let effective = resolve_effective_settings_for_folder_path(project_path.as_deref())
            .map_err(ApiError::Internal)?;

        let mut entries = Vec::new();
        for server in BuiltinCatalogService::server_definitions() {
            let config = lsp_config_record(&effective.settings, &server.server_definition_id);
            let status = resolve_server_status(server, &config, &env);
            entries.push(LspServerSettingsEntry {
                server: status,
                config,
            });
        }

        Ok(entries)
    }

    pub fn lsp_set_server_config(
        &self,
        config: LspServerConfigRecord,
    ) -> Result<LspServerConfigRecord, ApiError> {
        self.settings_update_global_section("lsp", Self::lsp_server_update(&config))?;
        Ok(config)
    }

    pub(crate) fn lsp_server_update(
        config: &LspServerConfigRecord,
    ) -> impl FnOnce(Value) -> Result<Value, ApiError> + '_ {
        move |mut lsp| {
            let servers = SettingsService::object_property(&mut lsp, "servers")?;
            for entry in servers.values_mut().filter_map(Value::as_object_mut) {
                entry.remove("runtime_path_override");
                entry.remove("runtime_args");
            }
            servers.insert(
                config.server_definition_id.clone(),
                serde_json::to_value(LspServerSettings {
                    enabled: config.enabled,
                    binary_path_override: config.binary_path_override.clone(),
                    extra_args: config.extra_args.clone(),
                    trace: config.trace,
                    settings: config.settings.clone(),
                })
                .map_err(|error| ApiError::Internal(error.to_string()))?,
            );
            Ok(lsp)
        }
    }

    pub fn lsp_start(
        &self,
        owner_id: Option<String>,
        session_id: String,
        folder_path: String,
        server_definition_id: String,
        root_path: String,
        events: EventSink<LspEvent>,
    ) -> Result<(), ApiError> {
        let server = BuiltinCatalogService::find_server_definition(&server_definition_id)
            .ok_or_else(|| {
                ApiError::NotFound(format!(
                    "Unknown LSP server definition {server_definition_id}"
                ))
            })?;

        let folder = resolve_folder(&folder_path)?;
        let folder_path = folder.to_string_lossy().into_owned();
        let root_path = normalize_root_path(&folder_path, &root_path)?;
        let effective = resolve_effective_settings_for_folder_path(Some(&folder))
            .map_err(ApiError::Internal)?;
        let config = lsp_config_record(&effective.settings, &server_definition_id);

        if !config.enabled {
            return Err(ApiError::InvalidArgument(format!(
                "LSP server {server_definition_id} is disabled"
            )));
        }

        let env = self.folder_env(&folder);
        let resolved =
            resolve_launch(server, &config, &env, &root_path).map_err(ApiError::Internal)?;

        self.lsp
            .spawn(session_id, owner_id, config.trace, resolved, events)
    }

    pub fn lsp_send(&self, session_id: String, message_json: String) -> Result<(), ApiError> {
        self.lsp
            .send(&session_id, &message_json)
            .map_err(ApiError::Internal)
    }

    pub fn lsp_stop(&self, session_id: String) -> Result<(), ApiError> {
        self.lsp_kill(&session_id).map_err(ApiError::Internal)
    }

    /// Blocking twin of `lsp_stop`, for holders of a lock or a `Drop` impl
    /// that must kill a server without an executor. Killing is synchronous
    /// anyway: the async signature exists only for the RPC table.
    pub fn lsp_kill(&self, session_id: &str) -> Result<(), String> {
        self.lsp.kill(session_id)
    }
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
            "LSP root must stay inside project {project_path}"
        )));
    }

    Ok(candidate.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sworm_protocol::settings::LspTraceLevel;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sworm-lsp-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn lsp_server_update_preserves_native_settings_and_strips_legacy_keys() {
        let root = temp_root("patch");
        std::fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        std::fs::write(
            &path,
            r#"{
  "future_top": true,
  "lsp": {
    "future_lsp": true,
    "servers": {
      "other": {
        "runtime_path_override": "/bin/node",
        "runtime_args": ["--legacy"],
        "future_server": true,
        "settings": { "native": { "runtime_args": ["keep"] } }
      },
      "dev.sworm.vtsls::vtsls": {
        "runtime_path_override": "/bin/node",
        "runtime_args": ["--legacy"]
      }
    }
  }
}
"#,
        )
        .expect("settings file written");

        let record = LspServerConfigRecord {
            server_definition_id: "dev.sworm.vtsls::vtsls".to_string(),
            enabled: true,
            binary_path_override: None,
            extra_args: vec!["--stdio".to_string()],
            trace: LspTraceLevel::Messages,
            settings: Some(json!({ "typescript": { "preferences": { "quoteStyle": "single" } } })),
        };

        SettingsService::update_section(&path, "lsp", Host::lsp_server_update(&record))
            .expect("patched");
        let patched = std::fs::read_to_string(&path).expect("patched read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(parsed["lsp"]["future_lsp"], json!(true));
        assert_eq!(
            parsed["lsp"]["servers"]["other"]["future_server"],
            json!(true)
        );
        assert_eq!(
            parsed["lsp"]["servers"]["other"]["settings"]["native"]["runtime_args"],
            json!(["keep"])
        );
        for entry in parsed["lsp"]["servers"].as_object().unwrap().values() {
            assert!(entry.get("runtime_path_override").is_none());
            assert!(entry.get("runtime_args").is_none());
        }
        assert_eq!(
            parsed["lsp"]["servers"]["dev.sworm.vtsls::vtsls"]["settings"]["typescript"]
                ["preferences"]["quoteStyle"],
            json!("single")
        );
        std::fs::remove_dir_all(root).expect("temp dir removed");
    }
}
