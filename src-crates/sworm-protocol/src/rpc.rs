use serde::{Deserialize, Serialize};

/// Protocol version selected during the QUIC TLS handshake.
pub const ALPN: &[u8] = b"sworm/1";
pub const DEFAULT_SERVER_PORT: u16 = 7420;
/// Maximum request size. Requests contain paths and pairing metadata, never file bodies.
pub const MAX_REQUEST_FRAME_BYTES: usize = 64 * 1024;
/// Maximum response frame size, including JSON encoding.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
/// Bound whole-file reads well below the response frame ceiling.
pub const MAX_REMOTE_FILE_BYTES: usize = 16 * 1024 * 1024;

/// One request per QUIC bidirectional stream. `project_path`/`path` are
/// absolute paths on the daemon host.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Request {
    Pair {
        token: String,
        name: String,
    },
    FilesReadDir {
        project_path: String,
        dir_path: String,
        show_hidden: bool,
    },
    FileRead {
        project_path: String,
        file_path: String,
    },
    GitGetSummary {
        path: String,
    },
}

/// Dynamic payload retained until the protocol gains operation-specific response variants.
pub type Response = Result<serde_json::Value, WireError>;

/// Wire mirror of `sworm_core::errors::ApiError` plus transport-level auth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireError {
    Database { message: String },
    Pty { message: String },
    Io { message: String },
    NotFound { message: String },
    InvalidArgument { message: String },
    Internal { message: String },
    BranchUnmerged { branch: String, message: String },
    DirtyWorktree { message: String },
    Unauthorized { message: String },
}
