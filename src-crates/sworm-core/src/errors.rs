use serde::{ser::SerializeStruct, Serialize};
use sworm_protocol::rpc::WireError;

/// Unified error type returned by host operations and application commands.
///
/// Implements `Serialize` so errors can cross IPC/RPC boundaries as structured JSON.
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Database error: {0}")]
    Database(String),

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

    #[error("Remote error: {0}")]
    Remote(String),

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

impl From<ApiError> for WireError {
    fn from(error: ApiError) -> Self {
        match error {
            ApiError::Database(message) => WireError::Database { message },
            ApiError::Pty(message) => WireError::Pty { message },
            ApiError::Io(message) => WireError::Io { message },
            ApiError::NotFound(message) => WireError::NotFound { message },
            ApiError::InvalidArgument(message) => WireError::InvalidArgument { message },
            ApiError::Internal(message) => WireError::Internal { message },
            ApiError::Remote(message) => WireError::Internal { message },
            ApiError::BranchUnmerged { branch, message } => {
                WireError::BranchUnmerged { branch, message }
            }
            ApiError::DirtyWorktree { message } => WireError::DirtyWorktree { message },
        }
    }
}

impl From<WireError> for ApiError {
    fn from(error: WireError) -> Self {
        match error {
            WireError::Database { message } => ApiError::Database(message),
            WireError::Pty { message } => ApiError::Pty(message),
            WireError::Io { message } => ApiError::Io(message),
            WireError::NotFound { message } => ApiError::NotFound(message),
            WireError::InvalidArgument { message } => ApiError::InvalidArgument(message),
            WireError::Internal { message } => ApiError::Internal(message),
            WireError::BranchUnmerged { branch, message } => {
                ApiError::BranchUnmerged { branch, message }
            }
            WireError::DirtyWorktree { message } => ApiError::DirtyWorktree { message },
            WireError::Unauthorized { message } => ApiError::Remote(message),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn branch_unmerged_round_trips_over_wire() {
        let wire = WireError::from(ApiError::BranchUnmerged {
            branch: "feature".to_string(),
            message: "merge it first".to_string(),
        });
        assert_eq!(
            wire,
            WireError::BranchUnmerged {
                branch: "feature".to_string(),
                message: "merge it first".to_string(),
            }
        );

        match ApiError::from(wire) {
            ApiError::BranchUnmerged { branch, message } => {
                assert_eq!(branch, "feature");
                assert_eq!(message, "merge it first");
            }
            error => panic!("expected BranchUnmerged, got {error:?}"),
        }
    }

    #[test]
    fn io_round_trips_over_wire() {
        let wire = WireError::from(ApiError::Io("disk full".to_string()));
        assert_eq!(
            wire,
            WireError::Io {
                message: "disk full".to_string(),
            }
        );

        match ApiError::from(wire) {
            ApiError::Io(message) => assert_eq!(message, "disk full"),
            error => panic!("expected Io, got {error:?}"),
        }
    }

    #[test]
    fn legacy_errors_still_serialize_as_strings() {
        assert_eq!(
            serde_json::to_value(ApiError::Io("disk full".to_string())).unwrap(),
            json!("IO error: disk full")
        );
    }

    #[test]
    fn structured_errors_keep_frontend_shape() {
        assert_eq!(
            serde_json::to_value(ApiError::BranchUnmerged {
                branch: "feature".to_string(),
                message: "merge it first".to_string(),
            })
            .unwrap(),
            json!({
                "kind": "branchUnmerged",
                "branch": "feature",
                "message": "merge it first",
            })
        );
        assert_eq!(
            serde_json::to_value(ApiError::DirtyWorktree {
                message: "commit first".to_string(),
            })
            .unwrap(),
            json!({
                "kind": "dirtyWorktree",
                "message": "commit first",
            })
        );
    }
}
