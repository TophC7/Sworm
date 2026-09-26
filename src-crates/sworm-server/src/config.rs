use anyhow::{anyhow, bail, ensure, Context};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};
pub use sworm_protocol::server_config::{ServerConfig, WebConfig, SERVER_CONFIG_FILE};
use sworm_remote::Identity;

pub fn default_path(config_dir: &Path) -> PathBuf {
    config_dir.join(SERVER_CONFIG_FILE)
}

/// A missing file means defaults: a fresh install needs no config.
pub fn load(path: &Path) -> anyhow::Result<ServerConfig> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ServerConfig::default())
        }
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    if raw.trim().is_empty() {
        return Ok(ServerConfig::default());
    }
    let value = jsonc_parser::parse_to_serde_value::<Value>(&raw, &Default::default())
        .map_err(|error| anyhow!("parse {}: {error}", path.display()))?;
    let config: ServerConfig = serde_json::from_value(value)
        .map_err(|error| anyhow!("invalid {}: {error}", path.display()))?;
    for (field, file) in [
        ("identity_file", &config.identity_file),
        ("authorized_keys_file", &config.authorized_keys_file),
    ] {
        if let Some(file) = file {
            ensure!(file.is_absolute(), "{field} must be an absolute path");
        }
    }
    Ok(config)
}

/// The daemon's TLS identity: the provisioned `identity_file`, else one
/// generated in the config dir on first use.
pub fn identity(config_dir: &Path, config: &ServerConfig) -> anyhow::Result<Identity> {
    Ok(match &config.identity_file {
        Some(path) => Identity::load(path)?,
        None => Identity::load_or_generate(config_dir, "server")?,
    })
}

/// `web.assets_dir` wins over the launcher-supplied default.
pub fn resolve_web_assets(
    web: &WebConfig,
    config_path: &Path,
    default: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let path = match (&web.assets_dir, default) {
        (Some(dir), _) => {
            ensure!(
                !dir.as_os_str().is_empty(),
                "web.assets_dir must not be empty"
            );
            config_path
                .parent()
                .map_or_else(|| dir.clone(), |base| base.join(dir))
        }
        (None, Some(dir)) => dir.to_path_buf(),
        (None, None) => {
            bail!("web.assets_dir is required; packaged launchers pass --web-assets-dir")
        }
    };
    ensure!(
        path.is_dir(),
        "web assets directory is invalid: {}",
        path.display()
    );
    let index = path.join("index.html");
    ensure!(
        index.is_file(),
        "web index.html is not a file: {}",
        index.display()
    );
    fs::File::open(&index)
        .with_context(|| format!("read web index.html at {}", index.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonc_config_resolves_relative_assets() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(SERVER_CONFIG_FILE);
        fs::create_dir_all(dir.path().join("web")).unwrap();
        fs::write(dir.path().join("web/index.html"), "<html></html>").unwrap();

        assert!(load(&config_path).unwrap().web.is_none());
        fs::write(
            &config_path,
            "{\n  // comments are fine\n  \"web\": { \"assets_dir\": \"web\" }\n}\n",
        )
        .unwrap();
        let config = load(&config_path).unwrap();
        let web = config.web.as_ref().unwrap();
        assert!(web.listen.ip().is_loopback());
        assert_eq!(
            resolve_web_assets(web, &config_path, Some(Path::new("/launcher/default"))).unwrap(),
            dir.path().join("web")
        );
    }

    #[test]
    fn relative_key_paths_and_unknown_fields_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join(SERVER_CONFIG_FILE);
        fs::write(&config_path, r#"{ "identity_file": "server.pem" }"#).unwrap();
        assert!(load(&config_path)
            .unwrap_err()
            .to_string()
            .contains("absolute"));
        fs::write(&config_path, r#"{ "auth_token": "inline" }"#).unwrap();
        assert!(load(&config_path)
            .unwrap_err()
            .to_string()
            .contains("unknown field"));
    }
}
