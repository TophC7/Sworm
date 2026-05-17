use crate::models::settings;
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

pub struct SettingsService;

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsJsoncLayer {
    pub path: PathBuf,
    pub loaded: bool,
    pub value: Value,
}

impl SettingsService {
    pub fn global_settings_path() -> Result<PathBuf, String> {
        Self::global_settings_path_from_env_vars(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )
    }

    pub fn global_shortcuts_path() -> Result<PathBuf, String> {
        Self::global_shortcuts_path_from_env_vars(
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("HOME"),
        )
    }

    pub fn global_settings_path_from_env_vars(
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
    ) -> Result<PathBuf, String> {
        Self::global_config_file_path_from_env_vars(
            xdg_config_home,
            home,
            settings::SETTINGS_FILE_NAME,
        )
    }

    pub fn global_shortcuts_path_from_env_vars(
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
    ) -> Result<PathBuf, String> {
        Self::global_config_file_path_from_env_vars(
            xdg_config_home,
            home,
            settings::SHORTCUTS_FILE_NAME,
        )
    }

    fn global_config_file_path_from_env_vars(
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
        file_name: &str,
    ) -> Result<PathBuf, String> {
        if let Some(path) = non_empty_env_path(xdg_config_home) {
            return Ok(path
                .join(settings::GLOBAL_SETTINGS_DIR_NAME)
                .join(file_name));
        }

        let home = non_empty_env_path(home)
            .ok_or_else(|| "HOME is required to resolve global config path".to_string())?;
        Ok(home
            .join(".config")
            .join(settings::GLOBAL_SETTINGS_DIR_NAME)
            .join(file_name))
    }

    pub fn project_settings_path(project_path: &Path) -> PathBuf {
        project_path.join(settings::PROJECT_SETTINGS_PATH)
    }

    pub fn ensure_global_settings_parent() -> Result<PathBuf, String> {
        let path = Self::global_settings_path()?;
        ensure_parent_dir(&path)?;
        Ok(path)
    }

    pub fn ensure_global_shortcuts_parent() -> Result<PathBuf, String> {
        let path = Self::global_shortcuts_path()?;
        ensure_parent_dir(&path)?;
        Ok(path)
    }

    pub fn ensure_project_settings_parent(project_path: &Path) -> Result<PathBuf, String> {
        let path = Self::project_settings_path(project_path);
        ensure_parent_dir(&path)?;
        Ok(path)
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

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create {}: {}", parent.display(), error))
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
    use std::ffi::OsString;
    use uuid::Uuid;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sworm-settings-{name}-{}", Uuid::new_v4()))
    }

    #[test]
    fn global_settings_path_prefers_xdg_config_home() {
        let path = SettingsService::global_settings_path_from_env_vars(
            Some(OsString::from("/tmp/xdg")),
            Some(OsString::from("/home/test")),
        )
        .expect("path resolves");
        assert_eq!(path, PathBuf::from("/tmp/xdg/sworm/settings.jsonc"));
    }

    #[test]
    fn global_shortcuts_path_uses_same_config_dir() {
        let path = SettingsService::global_shortcuts_path_from_env_vars(
            Some(OsString::from("/tmp/xdg")),
            Some(OsString::from("/home/test")),
        )
        .expect("path resolves");
        assert_eq!(path, PathBuf::from("/tmp/xdg/sworm/shortcuts.jsonc"));
    }

    #[test]
    fn global_settings_path_falls_back_to_home_config() {
        let path = SettingsService::global_settings_path_from_env_vars(
            None,
            Some(OsString::from("/home/test")),
        )
        .expect("path resolves");
        assert_eq!(
            path,
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
