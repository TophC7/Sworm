use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::nix_env::{NixDetection, NixEnvRecord, NixEnvStatus};
use crate::models::provider::ProviderStatus;
use crate::services::folders::resolve_folder;
use crate::services::nix::{NixDiagnostic, NixService};
use crate::services::settings_resolution::{
    provider_binary_overrides, resolve_effective_settings_for_folder_path,
};

/// Detect Nix files in a folder and return current selection.
#[tauri::command]
pub async fn nix_detect(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixDetection, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let folder_path = folder.to_string_lossy().into_owned();
    let db = state.db.read();

    let detected_files = NixService::detect(&folder_path);
    let selected = NixService::get(db.conn(), &folder_path).map_err(ApiError::Database)?;

    Ok(NixDetection {
        folder_path,
        detected_files,
        selected,
    })
}

/// Select a Nix file for a folder. Validates against detected files.
#[tauri::command]
pub async fn nix_select(
    folder_path: String,
    nix_file: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixEnvRecord, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let folder_path = folder.to_string_lossy().into_owned();

    let detected = NixService::detect(&folder_path);
    if !detected.iter().any(|f| f == &nix_file) {
        return Err(ApiError::InvalidArgument(format!(
            "Nix file '{}' not found in folder. Detected: {:?}",
            nix_file, detected
        )));
    }

    let db = state.db.write();
    NixService::select(db.conn(), &folder_path, &nix_file).map_err(ApiError::Database)
}

/// RAII guard that removes a folder path from the eval lock set on drop.
struct NixEvalGuard<'a> {
    locks: &'a parking_lot::Mutex<std::collections::HashSet<String>>,
    folder_path: String,
}

impl<'a> Drop for NixEvalGuard<'a> {
    fn drop(&mut self) {
        self.locks.lock().remove(&self.folder_path);
    }
}

/// Evaluate the selected Nix expression (async, potentially slow).
#[tauri::command]
pub async fn nix_evaluate(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixEnvRecord, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let folder_path = folder.to_string_lossy().into_owned();

    // Check and acquire evaluation lock
    {
        let mut locks = state.nix_eval_locks.lock();
        if locks.contains(&folder_path) {
            return Err(ApiError::InvalidArgument(
                "Nix evaluation already in progress for this folder".to_string(),
            ));
        }
        locks.insert(folder_path.clone());
    }

    // RAII guard ensures the lock is always released, even on early ? returns or panics
    let _guard = NixEvalGuard {
        locks: &state.nix_eval_locks,
        folder_path: folder_path.clone(),
    };

    // Load nix_file and user-configured timeout
    let (nix_file, timeout_secs) = {
        let db = state.db.write();
        let record = NixService::get(db.conn(), &folder_path)
            .map_err(ApiError::Database)?
            .ok_or_else(|| {
                ApiError::InvalidArgument(
                    "No Nix file selected for this folder. Call nix_select first.".to_string(),
                )
            })?;

        let effective_settings = resolve_effective_settings_for_folder_path(Some(&folder))
            .map_err(ApiError::Internal)?;
        // Clamp to a sane range so a bad config value can't hang the app forever or
        // fire a timeout before nix has even finished spawning.
        let timeout_secs = effective_settings
            .settings
            .general
            .nix_eval_timeout_secs
            .clamp(30, 3600);

        NixService::set_status(db.conn(), &folder_path, NixEnvStatus::Evaluating)
            .map_err(ApiError::Database)?;

        (record.nix_file, timeout_secs)
    };

    // Run evaluation on a blocking thread (can take 30+ seconds on a warm store,
    // many minutes on a cold one).
    let eval_folder_path = folder_path.clone();
    let eval_result = tokio::task::spawn_blocking(move || {
        NixService::evaluate(&eval_folder_path, &nix_file, timeout_secs)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("Evaluation task panicked: {}", e)))?;

    // Save result to DB and return the updated record in one lock
    let db = state.db.write();
    match eval_result {
        Ok(env_vars) => {
            NixService::save_success(db.conn(), &folder_path, &env_vars)
                .map_err(ApiError::Database)?;
        }
        Err(eval_error) => {
            NixService::save_error(db.conn(), &folder_path, &eval_error)
                .map_err(ApiError::Database)?;
            return Err(ApiError::Internal(eval_error.to_string()));
        }
    }

    // _guard drops here, releasing the eval lock

    NixService::get(db.conn(), &folder_path)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::Internal("Nix env record disappeared after save".to_string()))
}

/// Clear the Nix environment for a folder.
#[tauri::command]
pub async fn nix_clear(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let db = state.db.write();
    NixService::remove(db.conn(), &folder.to_string_lossy()).map_err(ApiError::Database)
}

/// Parse-check a Nix file and return diagnostics.
/// Joins folder_path + file_path server-side to avoid frontend path construction.
#[tauri::command]
pub async fn nix_lint(
    folder_path: String,
    file_path: String,
) -> Result<Vec<NixDiagnostic>, ApiError> {
    let abs_path = std::path::Path::new(&folder_path)
        .join(&file_path)
        .to_string_lossy()
        .to_string();
    tokio::task::spawn_blocking(move || NixService::lint_nix(&abs_path))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(ApiError::Internal)
}

/// Detect providers using the folder's Nix-augmented PATH.
#[tauri::command]
pub async fn provider_list_for_folder(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let db = state.db.read();

    let merged_path = match NixService::load_env_vars(db.conn(), &folder.to_string_lossy()) {
        Ok(Some(nix_env)) => NixService::merged_path(&state.env.merged_path, &nix_env),
        _ => state.env.merged_path.clone(),
    };
    drop(db);

    let effective =
        resolve_effective_settings_for_folder_path(Some(&folder)).map_err(ApiError::Internal)?;
    let overrides = provider_binary_overrides(&effective.settings);

    Ok(state
        .providers
        .detect_all(&merged_path, &overrides, Some(&state.env.detected_shell)))
}
