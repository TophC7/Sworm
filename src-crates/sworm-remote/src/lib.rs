pub mod client;
pub mod identity;
pub mod tls;
pub mod wire;

pub use client::RemoteClient;
pub use identity::{Fingerprint, Identity};

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("{0}")]
    Transport(String),
    #[error("{0}")]
    Connection(String),
    #[error("{0}")]
    Timeout(String),
    #[error("{0:?}")]
    Wire(sworm_protocol::rpc::WireError),
    #[error("{0}")]
    Identity(String),
}

impl RemoteError {
    pub fn transport(context: impl std::fmt::Display, error: impl std::fmt::Display) -> Self {
        Self::Transport(format!("{context}: {error}"))
    }

    pub fn identity(context: impl std::fmt::Display, error: impl std::fmt::Display) -> Self {
        Self::Identity(format!("{context}: {error}"))
    }
}
