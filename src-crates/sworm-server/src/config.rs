use anyhow::Context;
use serde::Deserialize;
use std::{
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use sworm_protocol::rpc::DEFAULT_SERVER_PORT;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ServerConfig {
    pub listen: SocketAddr,
    pub auth_token: Option<String>,
    auth_token_file: Option<PathBuf>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], DEFAULT_SERVER_PORT)),
            auth_token: None,
            auth_token_file: None,
        }
    }
}

pub(crate) fn load(config_dir: &Path) -> anyhow::Result<ServerConfig> {
    load_with_auth_token_file(
        config_dir,
        std::env::var_os("SWORM_SERVER_AUTH_TOKEN_FILE").map(PathBuf::from),
    )
}

fn load_with_auth_token_file(
    config_dir: &Path,
    override_file: Option<PathBuf>,
) -> anyhow::Result<ServerConfig> {
    let path = config_dir.join("server.toml");
    let mut config: ServerConfig = match fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ServerConfig::default(),
        Err(error) => return Err(error.into()),
    };

    if let Some(path) = override_file {
        // The service environment takes precedence over a legacy server.toml.
        config.auth_token = None;
        config.auth_token_file = Some(path);
    } else {
        anyhow::ensure!(
            config.auth_token.is_none() || config.auth_token_file.is_none(),
            "server.toml auth_token conflicts with auth_token_file"
        );
    }
    if let Some(path) = config.auth_token_file.as_deref() {
        anyhow::ensure!(
            path.is_absolute(),
            "auth_token_file must be an absolute path"
        );
        let token = fs::read_to_string(path)
            .with_context(|| format!("read auth_token_file {}", path.display()))?;
        config.auth_token = Some(token.trim_end_matches(['\r', '\n']).to_owned());
    }
    if config
        .auth_token
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        anyhow::bail!("server auth_token must not be empty");
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_file_resolves_without_server_toml() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        fs::write(&token_path, "secret-token\n").unwrap();

        let loaded = load_with_auth_token_file(dir.path(), Some(token_path)).unwrap();
        assert_eq!(loaded.auth_token.as_deref(), Some("secret-token"));
        assert!(load_with_auth_token_file(dir.path(), None)
            .unwrap()
            .auth_token
            .is_none());
    }

    #[test]
    fn token_file_failures_are_not_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let token_path = dir.path().join("token");
        let config_path = dir.path().join("server.toml");

        let missing = load_with_auth_token_file(dir.path(), Some(token_path.clone()))
            .err()
            .unwrap();
        assert!(missing.to_string().contains("read auth_token_file"));
        fs::write(&token_path, "\n").unwrap();
        let empty = load_with_auth_token_file(dir.path(), Some(token_path.clone()))
            .err()
            .unwrap();
        assert!(empty.to_string().contains("must not be empty"));

        fs::write(&token_path, "valid\n").unwrap();
        fs::write(&config_path, "auth_token = \"inline\"\n").unwrap();
        assert_eq!(
            load_with_auth_token_file(dir.path(), None)
                .unwrap()
                .auth_token
                .as_deref(),
            Some("inline")
        );
        let loaded = load_with_auth_token_file(dir.path(), Some(token_path.clone())).unwrap();
        assert_eq!(loaded.auth_token.as_deref(), Some("valid"));
        fs::write(
            &config_path,
            format!("auth_token_file = {:?}\n", token_path.display().to_string()),
        )
        .unwrap();
        let loaded = load_with_auth_token_file(dir.path(), None).unwrap();
        assert_eq!(loaded.auth_token.as_deref(), Some("valid"));
    }

    #[test]
    fn conflicting_toml_token_sources_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("server.toml"),
            "auth_token = \"inline\"\nauth_token_file = \"/some/token\"\n",
        )
        .unwrap();
        let error = load_with_auth_token_file(dir.path(), None).err().unwrap();
        assert!(error.to_string().contains("conflicts"));
    }
}
