use serde::Serialize;

/// Result of `session_start`: whether the provider resumed an existing
/// conversation (true) or started a new one (false).
#[derive(Debug, Clone, Serialize)]
pub struct SessionStartInfo {
    pub resumed: bool,
}
