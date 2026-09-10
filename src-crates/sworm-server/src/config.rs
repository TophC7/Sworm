use serde::Deserialize;
use std::{fs, net::SocketAddr, path::Path};
use sworm_protocol::rpc::DEFAULT_SERVER_PORT;

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ServerConfig {
    pub listen: SocketAddr,
    pub auth_token: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], DEFAULT_SERVER_PORT)),
            auth_token: None,
        }
    }
}

pub(crate) fn load(config_dir: &Path) -> anyhow::Result<ServerConfig> {
    let path = config_dir.join("server.toml");
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ServerConfig::default())
        }
        Err(error) => return Err(error.into()),
    };
    let config: ServerConfig = toml::from_str(&contents)?;
    if config
        .auth_token
        .as_deref()
        .is_some_and(|token| token.trim().is_empty())
    {
        anyhow::bail!("server.toml auth_token must not be empty");
    }
    Ok(config)
}
