use crate::errors::ApiError;
use crate::models::builtins::BuiltinCatalog;
use crate::services::builtins::BuiltinCatalogService;

#[tauri::command]
pub async fn builtins_get_catalog() -> Result<BuiltinCatalog, ApiError> {
    BuiltinCatalogService::catalog().map_err(ApiError::Internal)
}
