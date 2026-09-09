use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSchemaEntry {
    /// Stable identifier used by the frontend registry (e.g. `sworm.tasks`).
    pub id: String,

    /// Glob patterns matched against the opened file's URI.
    pub file_match: Vec<String>,

    /// JSON Schema object.
    pub schema: serde_json::Value,
}
