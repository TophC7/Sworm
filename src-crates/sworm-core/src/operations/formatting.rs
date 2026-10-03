use crate::{
    errors::ApiError,
    services::{folders::resolve_folder, formatting::FormattingService},
    Host,
};
use std::collections::HashMap;

impl Host {
    pub fn formatting_format_biome(
        &self,
        folder_path: String,
        file_path: String,
        content: String,
    ) -> Result<String, ApiError> {
        let (folder_path, env) = self.formatter_context(&folder_path)?;
        FormattingService::format_with_biome(&folder_path, &file_path, &content, &env)
            .map_err(ApiError::Internal)
    }

    pub fn formatting_format_nixfmt(
        &self,
        folder_path: String,
        content: String,
    ) -> Result<String, ApiError> {
        let (folder_path, env) = self.formatter_context(&folder_path)?;
        FormattingService::format_with_nixfmt(&folder_path, &content, &env)
            .map_err(ApiError::Internal)
    }

    /// Canonical folder path and child env, overlaid on the formatter's inherited env.
    fn formatter_context(
        &self,
        folder_path: &str,
    ) -> Result<(String, HashMap<String, String>), ApiError> {
        let folder = resolve_folder(folder_path)?;
        let env = self.folder_env(&folder);
        Ok((folder.to_string_lossy().into_owned(), env))
    }
}
