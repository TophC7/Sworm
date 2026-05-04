use serde::{ser::SerializeStruct, Serialize};

/// Unified error type returned by all Tauri commands.
///
/// Tauri requires command errors to implement `Serialize` so they can
/// cross the IPC boundary as structured JSON.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Keyring error: {0}")]
    Keyring(String),

    #[error("PTY error: {0}")]
    Pty(String),

    #[error("IO error: {0}")]
    Io(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Branch not fully merged: {branch}")]
    BranchUnmerged { branch: String, message: String },

    #[error("Working tree has uncommitted changes")]
    DirtyWorktree { message: String },
}

impl Serialize for ApiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if let ApiError::BranchUnmerged { branch, message } = self {
            let mut state = serializer.serialize_struct("ApiError", 3)?;
            state.serialize_field("kind", "branchUnmerged")?;
            state.serialize_field("branch", branch)?;
            state.serialize_field("message", message)?;
            return state.end();
        }

        if let ApiError::DirtyWorktree { message } = self {
            let mut state = serializer.serialize_struct("ApiError", 2)?;
            state.serialize_field("kind", "dirtyWorktree")?;
            state.serialize_field("message", message)?;
            return state.end();
        }

        // Keep legacy command errors as readable strings. Only typed
        // branches that frontend code matches directly serialize as objects.
        serializer.serialize_str(&self.to_string())
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::Database(e.to_string())
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::Io(e.to_string())
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}
