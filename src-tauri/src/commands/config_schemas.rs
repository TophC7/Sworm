// Command surface for project-scoped config schemas.
//
// Returns the full set of schema entries so the frontend can register
// them all with Monaco's JSON language service in one pass on boot.

use crate::services::config_schemas::{all_config_schemas, ConfigSchemaEntry};

#[tauri::command]
pub async fn config_schemas_list() -> Vec<ConfigSchemaEntry> {
    all_config_schemas()
}
