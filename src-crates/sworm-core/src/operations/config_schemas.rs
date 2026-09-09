use crate::{errors::ApiError, services::config_schemas::all_config_schemas, Host};
use sworm_protocol::config_schemas::ConfigSchemaEntry;

impl Host {
    pub async fn config_schemas_list(&self) -> Result<Vec<ConfigSchemaEntry>, ApiError> {
        all_config_schemas().map_err(ApiError::Internal)
    }
}
