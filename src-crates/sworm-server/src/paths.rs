use std::{ffi::OsString, path::PathBuf};
use sworm_protocol::settings::GLOBAL_SETTINGS_DIR_NAME;

pub fn default_data_dir() -> Result<PathBuf, String> {
    default_data_dir_from_env_vars(std::env::var_os("XDG_DATA_HOME"), std::env::var_os("HOME"))
}

fn default_data_dir_from_env_vars(
    xdg_data_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, String> {
    if let Some(path) = non_empty_path(xdg_data_home) {
        return Ok(path.join(GLOBAL_SETTINGS_DIR_NAME));
    }
    non_empty_path(home)
        .map(|path| path.join(".local/share").join(GLOBAL_SETTINGS_DIR_NAME))
        .ok_or_else(|| "HOME is required to resolve server data path".to_string())
}

fn non_empty_path(value: Option<OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}
