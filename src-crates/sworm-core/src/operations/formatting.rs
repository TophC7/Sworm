use crate::{
    errors::ApiError,
    services::{folders::resolve_folder, formatting::FormattingService, nix::NixService},
    Host,
};
use std::collections::HashMap;

impl Host {
    pub async fn formatting_format_biome(
        &self,
        folder_path: String,
        file_path: String,
        content: String,
    ) -> Result<String, ApiError> {
        let (folder_path, env) = self.formatter_context(&folder_path)?;
        tokio::task::spawn_blocking(move || {
            FormattingService::format_with_biome(&folder_path, &file_path, &content, &env)
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Internal)
    }

    pub async fn formatting_format_nixfmt(
        &self,
        folder_path: String,
        content: String,
    ) -> Result<String, ApiError> {
        let (folder_path, env) = self.formatter_context(&folder_path)?;
        tokio::task::spawn_blocking(move || {
            FormattingService::format_with_nixfmt(&folder_path, &content, &env)
        })
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(ApiError::Internal)
    }

    /// Canonical folder path plus host env overlaid with the folder's Nix env.
    fn formatter_context(
        &self,
        folder_path: &str,
    ) -> Result<(String, HashMap<String, String>), ApiError> {
        let folder_path = resolve_folder(folder_path)?.to_string_lossy().into_owned();
        let db = self.db.read();
        let host_env: HashMap<String, String> = std::env::vars().collect();
        let env =
            match NixService::load_env_vars(db.conn(), &folder_path).map_err(ApiError::Database)? {
                Some(nix_env) => NixService::merge_env(&host_env, &nix_env),
                None => host_env,
            };
        Ok((folder_path, env))
    }
}
