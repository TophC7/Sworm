use schemars::JsonSchema;
use serde::Deserialize;
use std::{net::SocketAddr, path::PathBuf};

pub const DEFAULT_SERVER_PORT: u16 = 7420;

pub const SERVER_CONFIG_FILE: &str = "server.jsonc";

/// `server.jsonc`: how `sworm-server` listens and admits clients. Separate from
/// `settings.jsonc`, which paired clients can edit over RPC.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// UDP `host:port` for desktop connections.
    pub listen: SocketAddr,
    /// Absolute path to a provisioned identity (certificate + private key PEM,
    /// see `sworm-server keygen`), like sshd's `HostKey`. Unset means
    /// `<config-dir>/server.pem`, generated on first start.
    pub identity_file: Option<PathBuf>,
    /// Absolute path to a read-only list of trusted client fingerprints, one
    /// `SHA256:<hex> [name]` per line, checked alongside paired clients.
    pub authorized_keys_file: Option<PathBuf>,
    /// Serves the browser frontend when present. It has no authentication and
    /// grants the server user's full authority: keep it on loopback.
    pub web: Option<WebConfig>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], DEFAULT_SERVER_PORT)),
            identity_file: None,
            authorized_keys_file: None,
            web: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct WebConfig {
    /// TCP `host:port` for the browser frontend.
    pub listen: SocketAddr,
    /// Built frontend directory; relative paths resolve against this file's
    /// directory. Packaged launchers supply a default.
    pub assets_dir: Option<PathBuf>,
    /// Origins (`http(s)://host[:port]`) trusted besides the page's own, for a
    /// frontend served elsewhere such as the Vite dev server. Anything listed
    /// gets the same unauthenticated authority as the page.
    pub allowed_origins: Vec<String>,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 7421)),
            assets_dir: None,
            allowed_origins: Vec::new(),
        }
    }
}
