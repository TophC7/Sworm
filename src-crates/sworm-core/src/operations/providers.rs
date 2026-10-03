use crate::{
    errors::ApiError,
    services::{
        folders::resolve_folder,
        settings_resolution::{
            provider_binary_overrides, resolve_effective_settings_for_folder_path,
        },
    },
    Host,
};
use sworm_protocol::provider::ProviderStatus;

impl Host {
    /// List all providers with their detection status.
    pub fn provider_list(&self) -> Result<Vec<ProviderStatus>, ApiError> {
        let effective =
            resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
        let overrides = provider_binary_overrides(&effective.settings);
        Ok(self
            .providers
            .detect_all(self.env.path(), &overrides, Some(&self.env.detected_shell)))
    }

    pub fn provider_list_for_folder(
        &self,
        folder_path: String,
    ) -> Result<Vec<ProviderStatus>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let env = self.folder_env(&folder);
        let effective = resolve_effective_settings_for_folder_path(Some(&folder))
            .map_err(ApiError::Internal)?;
        let overrides = provider_binary_overrides(&effective.settings);
        Ok(self
            .providers
            .detect_all(&env["PATH"], &overrides, Some(&self.env.detected_shell)))
    }
}
