use crate::errors::ApiError;
use crate::host::Host;
use crate::services::app_state_kv::AppStateKvService;
use crate::services::runtime_info::read_runtime_info;
use sworm_protocol::app::AppRuntimeInfo;

impl Host {
    pub async fn app_state_get(&self, key: String) -> Result<Option<String>, ApiError> {
        let db = self.db.read();
        AppStateKvService::new()
            .get(db.conn(), &key)
            .map_err(ApiError::Database)
    }

    pub async fn app_state_put(&self, key: String, value_json: String) -> Result<(), ApiError> {
        let db = self.db.write();
        AppStateKvService::new()
            .put(db.conn(), &key, &value_json)
            .map_err(ApiError::Database)
    }

    pub async fn app_state_delete(&self, key: String) -> Result<(), ApiError> {
        let db = self.db.write();
        AppStateKvService::new()
            .delete(db.conn(), &key)
            .map_err(ApiError::Database)
    }

    pub async fn app_runtime_info(
        &self,
        name: String,
        version: String,
    ) -> Result<AppRuntimeInfo, ApiError> {
        tokio::task::spawn_blocking(move || read_runtime_info(name, version))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))
    }
}
