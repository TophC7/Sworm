use crate::{errors::ApiError, services::builtins::BuiltinCatalogService, Host};
use sworm_protocol::builtins::BuiltinCatalog;

impl Host {
    pub async fn builtins_get_catalog(&self) -> Result<BuiltinCatalog, ApiError> {
        BuiltinCatalogService::catalog().map_err(ApiError::Internal)
    }
}
