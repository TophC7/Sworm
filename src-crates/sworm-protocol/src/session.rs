use serde::Serialize;

/// Result of `session_start`.
///
/// `resume_token` is the provider-owned conversation identity the
/// launched process is known to own *at spawn time*: Claude always has
/// one (resumed or freshly minted); Codex/Antigravity/OMP only when
/// resuming a validated token, otherwise `None` and post-spawn discovery
/// announces it later via `PtyEvent::ResumeTokenBound`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStartInfo {
    /// Whether the provider resumed an existing conversation.
    pub resumed: bool,
    pub resume_token: Option<String>,
}
