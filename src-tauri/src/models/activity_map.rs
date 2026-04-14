use serde::{Deserialize, Serialize};

/// Activity from a single provider in a discovered project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProviderActivity {
    /// Provider slug: "claude_code", "codex", "copilot", "gemini", "crush", "opencode"
    pub provider_id: String,
    /// ISO 8601 timestamp of most recent activity from this provider.
    pub last_active: String,
    /// Per-day session counts for the trailing 7 days.
    /// Index 0 = 6 days ago, index 6 = today.
    pub daily_counts: [u32; 7],
}

/// A project folder discovered from external agent conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProject {
    /// Absolute path to the project directory.
    pub path: String,
    /// Basename of the path (for display).
    pub name: String,
    /// Whether the path exists on disk (deleted projects get greyed out).
    pub path_exists: bool,
    /// Whether this path is already a Sworm project.
    pub is_sworm_project: bool,
    /// The Sworm project ID if already registered.
    pub sworm_project_id: Option<String>,
    /// ISO 8601 timestamp of most recent activity across all providers.
    pub last_active: String,
    /// Per-provider activity data.
    pub providers: Vec<DiscoveredProviderActivity>,
}
