use crate::{errors::ApiError, services::builtins::BuiltinCatalogService, Host};
use sworm_protocol::builtins::BuiltinCatalog;

impl Host {
    pub fn builtins_get_catalog(&self) -> Result<BuiltinCatalog, ApiError> {
        Ok(BuiltinCatalogService::catalog().clone())
    }
}
