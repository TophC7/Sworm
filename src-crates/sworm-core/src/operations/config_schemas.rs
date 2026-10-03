use crate::{errors::ApiError, services::config_schemas::all_config_schemas, Host};
use sworm_protocol::config_schemas::ConfigSchemaEntry;

impl Host {
    pub fn config_schemas_list(&self) -> Result<Vec<ConfigSchemaEntry>, ApiError> {
        Ok(all_config_schemas())
    }
}
