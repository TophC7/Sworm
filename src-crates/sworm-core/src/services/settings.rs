use parking_lot::{Mutex, MutexGuard};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    io::Write,
    path::{Path, PathBuf},
};
use sworm_protocol::settings;

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
        Self::global_config_dir_from_env_vars(xdg_config_home, home)
            .map(|directory| directory.join(file_name))
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

    pub fn ensure_folder_settings_parent(folder_path: &Path) -> Result<PathBuf, String> {
        let path = Self::folder_settings_path(folder_path);
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
