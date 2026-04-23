// Central registry of JSON Schemas for project-scoped config files.
//
// Each entry pairs a Rust-derived schema with the glob patterns that
// identify the file. The frontend fetches this list on boot and hands
// each entry to Monaco so that opening a matching file gets autocomplete,
// validation, and hover docs for free.
//
// Adding a new config file is a three-step change:
//   <> define the type with `#[derive(JsonSchema)]` under models/
//   <> append a `ConfigSchemaEntry` below
//   <> ship it; the frontend picks it up automatically on next boot

use schemars::schema_for;
use serde::Serialize;

use crate::models::task::TasksFile;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaEntry {
    /// Stable identifier used by the frontend registry (e.g. `sworm.tasks`).
    pub id: String,

    /// Glob patterns matched against the opened file's URI.
    pub file_match: Vec<String>,

    /// JSON Schema object. Produced by schemars from the Rust type
    /// so the schema never drifts from the deserialization contract.
    pub schema: serde_json::Value,
}

pub fn all_config_schemas() -> Vec<ConfigSchemaEntry> {
    vec![ConfigSchemaEntry {
        id: "sworm.tasks".into(),
        file_match: vec!["**/.sworm/tasks.json".into()],
        schema: serde_json::to_value(schema_for!(TasksFile)).expect("tasks schema serializes"),
    }]
}
