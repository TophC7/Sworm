use crate::{
    errors::ApiError,
    services::{
        folders::resolve_folder,
        nix::NixService,
        settings_resolution::{
            provider_binary_overrides, resolve_effective_settings_for_folder_path,
        },
    },
    Host,
};
use sworm_protocol::provider::ProviderStatus;

impl Host {
    /// List all providers with their detection status.
    pub async fn provider_list(&self) -> Result<Vec<ProviderStatus>, ApiError> {
        let effective =
            resolve_effective_settings_for_folder_path(None).map_err(ApiError::Internal)?;
        let overrides = provider_binary_overrides(&effective.settings);
        Ok(self.providers.detect_all(
            &self.env.merged_path,
            &overrides,
            Some(&self.env.detected_shell),
        ))
    }

    pub async fn provider_list_for_folder(
        &self,
        folder_path: String,
    ) -> Result<Vec<ProviderStatus>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let db = self.db.read();
        let merged_path = match NixService::load_env_vars(db.conn(), &folder.to_string_lossy()) {
            Ok(Some(nix_env)) => NixService::merged_path(&self.env.merged_path, &nix_env),
            _ => self.env.merged_path.clone(),
        };
        drop(db);
        let effective = resolve_effective_settings_for_folder_path(Some(&folder))
            .map_err(ApiError::Internal)?;
        let overrides = provider_binary_overrides(&effective.settings);
        Ok(self
            .providers
            .detect_all(&merged_path, &overrides, Some(&self.env.detected_shell)))
    }
}
