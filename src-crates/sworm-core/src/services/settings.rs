use crate::errors::ApiError;
use jsonc_parser::{
    cst::{CstInputValue, CstRootNode},
    ParseOptions,
};
use parking_lot::{Mutex, MutexGuard};
use serde_json::{json, Map, Value};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};
use sworm_protocol::settings::{self, SettingsLayerPayload};

pub struct SettingsService;

// All in-process settings read/modify/write operations share this lock.
static SETTINGS_MUTATION: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsJsoncLayer {
    pub path: PathBuf,
    pub loaded: bool,
    pub value: Value,
}

impl SettingsService {
    // ponytail: one lock for all settings paths; split per path if contention matters.
    pub(crate) fn mutation_lock() -> MutexGuard<'static, ()> {
        SETTINGS_MUTATION.lock()
    }

    /// Rename is the commit point; failures before it leave the destination intact.
    pub(crate) fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
        // Replace a symlink's target, not the link: home-manager and dotfiles
        // setups own the link. A missing file has nothing to resolve.
        let path = &std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let temporary = path.with_file_name(format!(".settings-{}.tmp", uuid::Uuid::new_v4()));
        let mut created = false;
        let result = (|| -> std::io::Result<()> {
            let parent = path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Settings path has no parent",
                )
            })?;
            std::fs::create_dir_all(parent)?;
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            created = true;
            file.write_all(contents.as_bytes())?;
            match std::fs::metadata(path) {
                Ok(metadata) => file.set_permissions(metadata.permissions())?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, path)
        })();
        if result.is_err() && created {
            let _ = std::fs::remove_file(&temporary);
        }
        result.map_err(|error| format!("Failed to write settings file {}: {error}", path.display()))
    }

    pub fn global_config_dir() -> Result<PathBuf, String> {
        Self::global_config_dir_from_env_vars(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )
    }

    pub fn global_settings_path() -> Result<PathBuf, String> {
        Ok(Self::global_config_dir()?.join(settings::SETTINGS_FILE_NAME))
    }

    pub fn global_shortcuts_path() -> Result<PathBuf, String> {
        Ok(Self::global_config_dir()?.join(settings::SHORTCUTS_FILE_NAME))
    }

    pub fn global_config_dir_from_env_vars(
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
    ) -> Result<PathBuf, String> {
        if let Some(path) = non_empty_env_path(xdg_config_home) {
            return Ok(path.join(settings::GLOBAL_SETTINGS_DIR_NAME));
        }

        let home = non_empty_env_path(home)
            .ok_or_else(|| "HOME is required to resolve global config path".to_string())?;
        Ok(home
            .join(".config")
            .join(settings::GLOBAL_SETTINGS_DIR_NAME))
    }

    pub fn folder_settings_path(folder_path: &Path) -> PathBuf {
        folder_path.join(settings::FOLDER_SETTINGS_PATH)
    }

    pub fn update_section(
        path: &Path,
        section: &str,
        update: impl FnOnce(Value) -> Result<Value, ApiError>,
    ) -> Result<SettingsLayerPayload, ApiError> {
        Self::validate_top_level_section(section)?;
        let _guard = Self::mutation_lock();
        let original = match std::fs::read_to_string(path) {
            Ok(original) => original,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => "{\n}\n".to_string(),
            Err(error) => {
                return Err(ApiError::Io(format!(
                    "Failed to read settings file {}: {error}",
                    path.display()
                )))
            }
        };
        let mut root = if original.trim().is_empty() {
            json!({})
        } else {
            jsonc_parser::parse_to_serde_value::<Value>(&original, &Default::default())
                .map_err(|error| ApiError::Internal(format!("Invalid settings JSONC: {error}")))?
        };
        let previous = root
            .get_mut(section)
            .map(Value::take)
            .unwrap_or(Value::Null);
        if root.is_null() {
            root = json!({});
        }
        let value = update(previous)?;
        let patched =
            patch_top_level_section(&original, section, &value).map_err(ApiError::Internal)?;
        root.as_object_mut()
            .ok_or_else(|| {
                ApiError::InvalidArgument("Settings root must be an object".to_string())
            })?
            .insert(section.to_string(), value);
        Self::write_atomic(path, &patched).map_err(ApiError::Io)?;
        Ok(SettingsLayerPayload {
            path: path.to_string_lossy().into_owned(),
            loaded: true,
            value: root,
            diagnostics: Vec::new(),
        })
    }

    pub fn ensure_file(path: &Path) -> Result<(), ApiError> {
        let _guard = Self::mutation_lock();
        if path.exists() {
            return Ok(());
        }
        Self::write_atomic(path, "{\n}\n").map_err(ApiError::Io)
    }

    pub fn object_property<'a>(
        root: &'a mut Value,
        key: &str,
    ) -> Result<&'a mut Map<String, Value>, ApiError> {
        if root.is_null() {
            *root = json!({});
        }
        let object = root.as_object_mut().ok_or_else(|| {
            ApiError::InvalidArgument("Settings root must be an object".to_string())
        })?;
        let value = object.entry(key.to_string()).or_insert_with(|| json!({}));
        if value.is_null() {
            *value = json!({});
        }
        value.as_object_mut().ok_or_else(|| {
            ApiError::InvalidArgument(format!("Settings `{key}` section must be an object"))
        })
    }

    fn validate_top_level_section(section: &str) -> Result<(), ApiError> {
        if matches!(
            section,
            "window"
                | "terminal"
                | "nix"
                | "explorer"
                | "formatting"
                | "providers"
                | "lsp"
                | "remotes"
        ) {
            Ok(())
        } else {
            Err(ApiError::InvalidArgument(format!(
                "Unsupported settings section `{section}`"
            )))
        }
    }

    pub fn read_jsonc_layer_or_empty(path: &Path) -> Result<SettingsJsoncLayer, String> {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SettingsJsoncLayer {
                    path: path.to_path_buf(),
                    loaded: false,
                    value: json!({}),
                });
            }
            Err(error) => {
                return Err(format!(
                    "Failed to read settings file {}: {}",
                    path.display(),
                    error
                ));
            }
        };

        let value = if raw.trim().is_empty() {
            json!({})
        } else {
            jsonc_parser::parse_to_serde_value::<Value>(&raw, &Default::default()).map_err(
                |error| {
                    format!(
                        "Failed to parse settings file {}: {}",
                        path.display(),
                        error
                    )
                },
            )?
        };

        Ok(SettingsJsoncLayer {
            path: path.to_path_buf(),
            loaded: true,
            value,
        })
    }
}

fn patch_top_level_section(original: &str, section: &str, value: &Value) -> Result<String, String> {
    let root = CstRootNode::parse(original, &ParseOptions::default())
        .map_err(|error| format!("Failed to parse settings JSONC: {error}"))?;
    let object = root
        .object_value_or_create()
        .ok_or_else(|| "Settings JSONC root must be an object".to_string())?;
    let input = serde_value_to_cst_input(value)?;

    match object.get(section) {
        Some(property) => property.set_value(input),
        None => {
            object.append(section, input);
        }
    }

    Ok(root.to_string())
}

fn serde_value_to_cst_input(value: &Value) -> Result<CstInputValue, String> {
    Ok(match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(value) => CstInputValue::Bool(*value),
        Value::Number(value) => CstInputValue::Number(value.to_string()),
        Value::String(value) => CstInputValue::String(value.clone()),
        Value::Array(values) => CstInputValue::Array(
            values
                .iter()
                .map(serde_value_to_cst_input)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::Object(values) => CstInputValue::Object(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), serde_value_to_cst_input(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}
fn non_empty_env_path(value: Option<OsString>) -> Option<PathBuf> {
    let value = value?;
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Host;
    use std::{ffi::OsString, fs};
    use sworm_protocol::settings::{LspServerConfigRecord, LspTraceLevel};
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sworm-settings-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn failed_atomic_replace_keeps_destination_and_removes_temporary() {
        let root = temp_root("atomic-failure");
        let path = root.join("settings.jsonc");
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("original"), "untouched").unwrap();
        assert!(SettingsService::write_atomic(&path, "replacement").is_err());
        assert_eq!(
            std::fs::read_to_string(path.join("original")).unwrap(),
            "untouched"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_preserves_private_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let root = temp_root("atomic-permissions");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.jsonc");
        SettingsService::write_atomic(&path, "initial").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        SettingsService::write_atomic(&path, "replacement").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_replace_writes_through_symlink() {
        let root = temp_root("atomic-symlink");
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("managed.jsonc");
        let link = root.join("settings.jsonc");
        std::fs::write(&target, "initial").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();
        SettingsService::write_atomic(&link, "replacement").unwrap();
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "replacement");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn patch_preserves_unknown_keys_and_untouched_comments() {
        let original = r#"{
  // Provider comment must stay.
  "providers": {
    "claude_code": { "enabled": true }
  },
  "future_top": { "keep": true },
  "general": {
    // Touched-section comments may be lost.
    "terminal_font_size": 13
  }
}
"#;

        let patched = patch_top_level_section(
            original,
            "general",
            &json!({
                "terminal_font_family": "Iosevka",
                "terminal_font_size": 15
            }),
        )
        .expect("patch succeeds");

        assert!(patched.contains("Provider comment must stay"));
        assert!(patched.contains("future_top"));
        assert!(patched.contains("Iosevka"));
        assert!(patched.contains("terminal_font_size"));

        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched JSONC parses");
        assert_eq!(parsed["future_top"]["keep"], json!(true));
        assert_eq!(parsed["general"]["terminal_font_size"], json!(15));
    }

    #[test]
    fn patch_adds_missing_top_level_section() {
        let patched = patch_top_level_section(
            r#"{
  "future_top": true
}
"#,
            "formatting",
            &json!({ "json": { "formatter": "biome" } }),
        )
        .expect("patch succeeds");

        assert!(patched.contains("future_top"));
        assert!(patched.contains("formatting"));

        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched JSONC parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(parsed["formatting"]["json"]["formatter"], json!("biome"));
    }

    #[test]
    fn patch_creates_object_for_empty_file() {
        let patched = patch_top_level_section("", "general", &json!({ "theme": "system" }))
            .expect("patch succeeds");

        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched JSONC parses");
        assert_eq!(parsed["general"]["theme"], json!("system"));
    }

    #[test]
    fn patch_rejects_non_object_root() {
        let error = patch_top_level_section("[]", "general", &json!({ "theme": "system" }))
            .expect_err("non-object root rejected");

        assert!(error.contains("root must be an object"));
    }
    #[test]
    fn concurrent_section_updates_keep_every_entry() {
        let root = temp_root("concurrent");
        let path = root.join("settings.jsonc");
        let barrier = std::sync::Barrier::new(16);
        std::thread::scope(|scope| {
            for index in 0..8 {
                let path = &path;
                let barrier = &barrier;
                scope.spawn(move || {
                    let config = LspServerConfigRecord {
                        server_definition_id: format!("server-{index}"),
                        enabled: true,
                        binary_path_override: None,
                        extra_args: vec![format!("--writer-{index}")],
                        trace: LspTraceLevel::Messages,
                        settings: Some(json!({ "writer": index })),
                    };
                    barrier.wait();
                    SettingsService::update_section(path, "lsp", Host::lsp_server_update(&config))
                        .unwrap();
                });
                scope.spawn(move || {
                    barrier.wait();
                    SettingsService::update_section(path, "remotes", |remotes| {
                        let mut root = json!({ "remotes": remotes });
                        let entries = SettingsService::object_property(&mut root, "remotes")?;
                        entries.insert(
                            index.to_string(),
                            json!({ "address": format!("host-{index}:7420") }),
                        );
                        Ok(Value::Object(std::mem::take(entries)))
                    })
                    .unwrap();
                });
            }
        });
        let layer = SettingsService::read_jsonc_layer_or_empty(&path).unwrap();
        for index in 0..8 {
            assert_eq!(
                layer.value["lsp"]["servers"][format!("server-{index}")]["settings"]["writer"],
                json!(index)
            );
            assert_eq!(
                layer.value["remotes"][index.to_string()]["address"],
                format!("host-{index}:7420")
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejected_mutation_leaves_settings_unchanged() {
        let root = temp_root("rejected");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.jsonc");
        let original = "{ /* keep */ \"remotes\": { \"existing\": {} } }\n";
        fs::write(&path, original).unwrap();
        let result = SettingsService::update_section(&path, "remotes", |_| {
            Err(ApiError::InvalidArgument("duplicate remote".to_string()))
        });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validates_known_top_level_sections() {
        for section in [
            "window",
            "terminal",
            "nix",
            "explorer",
            "formatting",
            "providers",
            "lsp",
            "remotes",
        ] {
            SettingsService::validate_top_level_section(section).expect("section valid");
        }
        assert!(SettingsService::validate_top_level_section("nope").is_err());
    }
    #[test]
    fn ensure_folder_settings_file_creates_parent_and_file() {
        let root = temp_root("folder-file");
        fs::create_dir_all(&root).expect("folder root created");
        let path = SettingsService::folder_settings_path(&root);
        SettingsService::ensure_file(&path).expect("settings file created");
        assert_eq!(path, root.join(".sworm/settings.jsonc"));
        assert_eq!(fs::read_to_string(&path).expect("file read"), "{\n}\n");
        fs::remove_dir_all(root).expect("temp dir removed");
    }

    #[test]
    fn update_section_preserves_unknown_top_level() {
        let root = temp_root("patch-section");
        fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        fs::write(
            &path,
            "{\n  \"future_top\": true,\n  \"terminal\": { \"font_size\": 13 }\n}\n",
        )
        .expect("settings file written");
        SettingsService::update_section(&path, "terminal", |_| Ok(json!({ "font_size": 16 })))
            .expect("section patched");
        let patched = fs::read_to_string(&path).expect("patched file read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["future_top"], json!(true));
        assert_eq!(parsed["terminal"]["font_size"], json!(16));
        fs::remove_dir_all(root).expect("temp dir removed");
    }

    #[test]
    fn object_property_update_preserves_sibling_entries() {
        let root = temp_root("patch-provider");
        fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        fs::write(
            &path,
            "{\n  \"providers\": {\n    \"codex\": { \"enabled\": false }\n  }\n}\n",
        )
        .expect("settings file written");
        SettingsService::update_section(&path, "providers", |providers| {
            let mut root = json!({ "providers": providers });
            let entries = SettingsService::object_property(&mut root, "providers")?;
            entries.insert(
                "claude_code".to_string(),
                json!({ "binary_path_override": "/bin/claude" }),
            );
            Ok(Value::Object(std::mem::take(entries)))
        })
        .expect("provider patched");
        let patched = fs::read_to_string(&path).expect("patched file read");
        let parsed = jsonc_parser::parse_to_serde_value::<Value>(&patched, &Default::default())
            .expect("patched parses");
        assert_eq!(parsed["providers"]["codex"]["enabled"], json!(false));
        assert_eq!(
            parsed["providers"]["claude_code"]["binary_path_override"],
            json!("/bin/claude")
        );
        fs::remove_dir_all(root).expect("temp dir removed");
    }
    #[test]
    fn global_settings_path_prefers_xdg_config_home() {
        let directory = SettingsService::global_config_dir_from_env_vars(
            Some(OsString::from("/tmp/xdg")),
            Some(OsString::from("/home/test")),
        )
        .expect("path resolves");
        assert_eq!(directory, PathBuf::from("/tmp/xdg/sworm"));
        assert_eq!(
            directory.join(settings::SETTINGS_FILE_NAME),
            PathBuf::from("/tmp/xdg/sworm/settings.jsonc")
        );
    }

    #[test]
    fn global_shortcuts_path_uses_same_config_dir() {
        let directory = SettingsService::global_config_dir_from_env_vars(
            Some(OsString::from("/tmp/xdg")),
            Some(OsString::from("/home/test")),
        )
        .expect("path resolves");
        assert_eq!(
            directory.join(settings::SHORTCUTS_FILE_NAME),
            PathBuf::from("/tmp/xdg/sworm/shortcuts.jsonc")
        );
    }

    #[test]
    fn global_settings_path_falls_back_to_home_config() {
        let directory = SettingsService::global_config_dir_from_env_vars(
            None,
            Some(OsString::from("/home/test")),
        )
        .expect("path resolves");
        assert_eq!(
            directory.join(settings::SETTINGS_FILE_NAME),
            PathBuf::from("/home/test/.config/sworm/settings.jsonc")
        );
    }

    #[test]
    fn missing_jsonc_layer_is_empty_unloaded_object() {
        let path = temp_root("missing").join("settings.jsonc");
        let layer = SettingsService::read_jsonc_layer_or_empty(&path).expect("missing is empty");
        assert_eq!(layer.path, path);
        assert!(!layer.loaded);
        assert_eq!(layer.value, json!({}));
        assert!(!path.exists());
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn jsonc_layer_accepts_comments() {
        let root = temp_root("comments");
        std::fs::create_dir_all(&root).expect("temp dir created");
        let path = root.join("settings.jsonc");
        std::fs::write(
            &path,
            "{\n  // comment\n  \"general\": { \"theme\": \"system\" }\n}\n",
        )
        .expect("settings file written");

        let layer = SettingsService::read_jsonc_layer_or_empty(&path).expect("jsonc parses");
        assert!(layer.loaded);
        assert_eq!(layer.value["general"]["theme"], "system");
    }
}
