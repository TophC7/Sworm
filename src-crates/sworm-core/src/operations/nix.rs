use crate::{
    errors::ApiError,
    events::HostEvent,
    services::{
        folders::resolve_folder, nix::NixService,
        settings_resolution::resolve_effective_settings_for_folder_path,
    },
    Host,
};
use sworm_protocol::nix_env::{NixDetection, NixDiagnostic, NixEnvRecord, NixEnvStatus};

struct NixEvalGuard<'a> {
    locks: &'a parking_lot::Mutex<std::collections::HashSet<String>>,
    folder_path: String,
}

impl Drop for NixEvalGuard<'_> {
    fn drop(&mut self) {
        self.locks.lock().remove(&self.folder_path);
    }
}

impl Host {
    fn emit_nix_changed(&self, folder_path: &str) -> Result<(), ApiError> {
        (self.events)(HostEvent::NixChanged(folder_path.to_owned())).map_err(ApiError::Internal)
    }

    pub async fn nix_detect(&self, folder_path: String) -> Result<NixDetection, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let folder_path = folder.to_string_lossy().into_owned();
        let db = self.db.read();
        let detected_files = NixService::detect(&folder_path);
        let selected = NixService::get(db.conn(), &folder_path).map_err(ApiError::Database)?;
        Ok(NixDetection {
            folder_path,
            detected_files,
            selected,
        })
    }

    pub async fn nix_select(
        &self,
        folder_path: String,
        nix_file: String,
    ) -> Result<NixEnvRecord, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let folder_path = folder.to_string_lossy().into_owned();
        let detected = NixService::detect(&folder_path);
        if !detected.iter().any(|file| file == &nix_file) {
            return Err(ApiError::InvalidArgument(format!(
                "Nix file '{}' not found in folder. Detected: {:?}",
                nix_file, detected
            )));
        }

        let record = {
            let db = self.db.write();
            NixService::select(db.conn(), &folder_path, &nix_file).map_err(ApiError::Database)?
        };
        self.emit_nix_changed(&folder_path)?;
        Ok(record)
    }

    pub async fn nix_evaluate(&self, folder_path: String) -> Result<NixEnvRecord, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let folder_path = folder.to_string_lossy().into_owned();

        {
            let mut locks = self.nix_eval_locks.lock();
            if locks.contains(&folder_path) {
                return Err(ApiError::InvalidArgument(
                    "Nix evaluation already in progress for this folder".to_string(),
                ));
            }
            locks.insert(folder_path.clone());
        }
        let _guard = NixEvalGuard {
            locks: &self.nix_eval_locks,
            folder_path: folder_path.clone(),
        };

        let (nix_file, timeout_secs) = {
            let db = self.db.write();
            let record = NixService::get(db.conn(), &folder_path)
                .map_err(ApiError::Database)?
                .ok_or_else(|| {
                    ApiError::InvalidArgument(
                        "No Nix file selected for this folder. Call nix_select first.".to_string(),
                    )
                })?;
            let effective_settings = resolve_effective_settings_for_folder_path(Some(&folder))
                .map_err(ApiError::Internal)?;
            let timeout_secs = effective_settings
                .settings
                .nix
                .eval_timeout_secs
                .clamp(30, 3600);
            NixService::set_status(db.conn(), &folder_path, NixEnvStatus::Evaluating)
                .map_err(ApiError::Database)?;
            (record.nix_file, timeout_secs)
        };

        let eval_folder_path = folder_path.clone();
        let eval_result = tokio::task::spawn_blocking(move || {
            NixService::evaluate(&eval_folder_path, &nix_file, timeout_secs)
        })
        .await
        .map_err(|error| ApiError::Internal(format!("Evaluation task panicked: {}", error)))?;

        let db = self.db.write();
        match eval_result {
            Ok(env_vars) => {
                NixService::save_success(db.conn(), &folder_path, &env_vars)
                    .map_err(ApiError::Database)?;
            }
            Err(eval_error) => {
                NixService::save_error(db.conn(), &folder_path, &eval_error)
                    .map_err(ApiError::Database)?;
                drop(db);
                self.emit_nix_changed(&folder_path)?;
                return Err(ApiError::Internal(eval_error.to_string()));
            }
        }

        let record = NixService::get(db.conn(), &folder_path)
            .map_err(ApiError::Database)?
            .ok_or_else(|| {
                ApiError::Internal("Nix env record disappeared after save".to_string())
            })?;
        drop(db);
        self.emit_nix_changed(&folder_path)?;
        Ok(record)
    }

    pub async fn nix_clear(&self, folder_path: String) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let folder_path = folder.to_string_lossy().into_owned();
        {
            let db = self.db.write();
            NixService::remove(db.conn(), &folder_path).map_err(ApiError::Database)?;
        }
        self.emit_nix_changed(&folder_path)
    }

    pub async fn nix_lint(
        &self,
        folder_path: String,
        file_path: String,
    ) -> Result<Vec<NixDiagnostic>, ApiError> {
        let abs_path = std::path::Path::new(&folder_path)
            .join(&file_path)
            .to_string_lossy()
            .to_string();
        tokio::task::spawn_blocking(move || NixService::lint_nix(&abs_path))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
            .map_err(ApiError::Internal)
    }
}
