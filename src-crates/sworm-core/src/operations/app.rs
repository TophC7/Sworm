use crate::errors::ApiError;
use crate::host::Host;
use crate::services::app_state_kv::AppStateKvService;
use crate::services::runtime_info::read_runtime_info;
use sworm_protocol::app::AppRuntimeInfo;

impl Host {
    pub fn app_state_get(&self, key: String) -> Result<Option<String>, ApiError> {
        let db = self.db.read();
        AppStateKvService::get(db.conn(), &key).map_err(ApiError::Database)
    }

    pub fn app_state_put(&self, key: String, value_json: String) -> Result<(), ApiError> {
        let db = self.db.write();
        AppStateKvService::put(db.conn(), &key, &value_json).map_err(ApiError::Database)
    }

    pub fn app_state_delete(&self, key: String) -> Result<(), ApiError> {
        let db = self.db.write();
        AppStateKvService::delete(db.conn(), &key).map_err(ApiError::Database)
    }

    pub fn app_runtime_info(
        &self,
        name: String,
        version: String,
    ) -> Result<AppRuntimeInfo, ApiError> {
        Ok(read_runtime_info(name, version))
    }
}
