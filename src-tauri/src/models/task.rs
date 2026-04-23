// Per-project task definitions parsed from `.sworm/tasks.json`.
//
// Doc comments on each field become `description` entries in the
// generated JSON Schema, which Monaco surfaces as hover tooltips and
// autocomplete docs in the editor. Keep them user-facing.

use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Root shape of `.sworm/tasks.json`. Committed to the repo and
/// shared by the whole team.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TasksFile {
    /// Schema version. Current: 1.
    pub version: u32,

    /// Task entries shown in the launcher, command palette, and title-bar menu.
    pub tasks: Vec<TaskDefinition>,
}

/// A single reusable terminal command exposed as a task.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TaskDefinition {
    /// Stable identifier. Used for state keys and references.
    /// Keep it URL-safe (letters, digits, dashes).
    pub id: String,

    /// Display name shown on the tab and in menus.
    pub label: String,

    /// Shell command to execute. Runs via `$SHELL -c`, so pipes,
    /// globs, and environment expansion are supported.
    pub command: String,

    /// Working directory relative to the project root. Defaults to
    /// the project root when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,

    /// Additional environment variables merged on top of the
    /// inherited project environment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Lucide icon name used on the tab. Any valid Lucide identifier
    /// is accepted; invalid names fall back to a default task icon.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,

    /// Optional group label. Tasks sharing a group appear together
    /// in the launcher.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    /// If true, rerunning the task while it is active focuses the
    /// existing tab instead of opening a new one.
    #[serde(default)]
    pub singleton: bool,

    /// If true, terminal scrollback is cleared on rerun. Only
    /// meaningful for singleton tasks.
    #[serde(default)]
    pub clear_on_rerun: bool,

    /// If true, prompt the user for confirmation before running.
    /// Use for destructive or expensive commands.
    #[serde(default)]
    pub confirm: bool,
}
