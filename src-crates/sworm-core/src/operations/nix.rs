use crate::{
    errors::ApiError,
    events::HostEvent,
    services::{
        folders::resolve_folder, nix::NixService,
        settings_resolution::resolve_effective_settings_for_folder_path,
    },
    Host,
};
use std::collections::HashMap;
use std::path::Path;
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
    /// Resolve one child environment; unavailable Nix state falls back to the host.
    pub(crate) fn folder_env(&self, folder: &Path) -> HashMap<String, String> {
        let nix = {
            let db = self.db.read();
            NixService::load_env_vars(db.conn(), &folder.to_string_lossy())
        };
        match nix {
            Ok(nix) => self.env.with_nix(nix.as_ref()),
            Err(error) => {
                tracing::warn!(
                    "Failed to load Nix env for folder {}: {error}; using host environment",
                    folder.display()
                );
                self.env.with_nix(None)
            }
        }
    }

    pub fn nix_detect(&self, folder_path: String) -> Result<NixDetection, ApiError> {
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

    pub fn nix_select(
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
        self.emit(HostEvent::NixChanged(folder_path));
        Ok(record)
    }

    pub fn nix_evaluate(&self, folder_path: String) -> Result<NixEnvRecord, ApiError> {
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

        let timeout_secs = resolve_effective_settings_for_folder_path(Some(&folder))
            .map_err(ApiError::Internal)?
            .settings
            .nix
            .eval_timeout_secs
            .clamp(30, 3600);
        let nix_file = {
            let db = self.db.write();
            let record = NixService::get(db.conn(), &folder_path)
                .map_err(ApiError::Database)?
                .ok_or_else(|| {
                    ApiError::InvalidArgument(
                        "No Nix file selected for this folder. Call nix_select first.".to_string(),
                    )
                })?;
            NixService::set_status(db.conn(), &folder_path, NixEnvStatus::Evaluating)
                .map_err(ApiError::Database)?;
            record.nix_file
        };

        let eval_result = NixService::evaluate(&folder_path, &nix_file, timeout_secs);

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
                self.emit(HostEvent::NixChanged(folder_path));
                return Err(ApiError::Internal(eval_error.to_string()));
            }
        }

        let record = NixService::get(db.conn(), &folder_path)
            .map_err(ApiError::Database)?
            .ok_or_else(|| {
                ApiError::Internal("Nix env record disappeared after save".to_string())
            })?;
        drop(db);
        self.emit(HostEvent::NixChanged(folder_path));
        Ok(record)
    }

    pub fn nix_clear(&self, folder_path: String) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let folder_path = folder.to_string_lossy().into_owned();
        {
            let db = self.db.write();
            NixService::remove(db.conn(), &folder_path).map_err(ApiError::Database)?;
        }
        self.emit(HostEvent::NixChanged(folder_path));
        Ok(())
    }

    pub fn nix_lint(
        &self,
        folder_path: String,
        file_path: String,
    ) -> Result<Vec<NixDiagnostic>, ApiError> {
        let abs_path = std::path::Path::new(&folder_path)
            .join(&file_path)
            .to_string_lossy()
            .to_string();
        NixService::lint_nix(&abs_path).map_err(ApiError::Internal)
    }
}
