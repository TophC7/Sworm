use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::nix_env::{NixDetection, NixEnvRecord, NixEnvStatus};
use crate::models::provider::ProviderStatus;
use crate::services::nix::{NixDiagnostic, NixService};
use crate::services::settings::SettingsService;

/// Detect Nix files in a project directory and return current selection.
#[tauri::command]
pub fn nix_detect(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixDetection, ApiError> {
    let db = state.db.lock();
    let project = state
        .projects
        .get(db.conn(), &project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

    let detected_files = NixService::detect(&project.path);
    let selected = NixService::get(db.conn(), &project_id).map_err(ApiError::Database)?;

    Ok(NixDetection {
        project_id,
        project_path: project.path,
        detected_files,
        selected,
    })
}

/// Select a Nix file for a project. Validates against detected files.
#[tauri::command]
pub fn nix_select(
    project_id: String,
    nix_file: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixEnvRecord, ApiError> {
    let db = state.db.lock();

    let project = state
        .projects
        .get(db.conn(), &project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

    // Validate nix_file exists in the project directory
    let detected = NixService::detect(&project.path);
    if !detected.iter().any(|f| f == &nix_file) {
        return Err(ApiError::InvalidArgument(format!(
            "Nix file '{}' not found in project. Detected: {:?}",
            nix_file, detected
        )));
    }

    NixService::select(db.conn(), &project_id, &nix_file).map_err(ApiError::Database)
}

/// RAII guard that removes a project_id from the eval lock set on drop.
struct NixEvalGuard<'a> {
    locks: &'a parking_lot::Mutex<std::collections::HashSet<String>>,
    project_id: String,
}

impl<'a> Drop for NixEvalGuard<'a> {
    fn drop(&mut self) {
        self.locks.lock().remove(&self.project_id);
    }
}

/// Evaluate the selected Nix expression (async, potentially slow).
#[tauri::command]
pub async fn nix_evaluate(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<NixEnvRecord, ApiError> {
    // Check and acquire evaluation lock
    {
        let mut locks = state.nix_eval_locks.lock();
        if locks.contains(&project_id) {
            return Err(ApiError::InvalidArgument(
                "Nix evaluation already in progress for this project".to_string(),
            ));
        }
        locks.insert(project_id.clone());
    }

    // RAII guard ensures the lock is always released, even on early ? returns or panics
    let _guard = NixEvalGuard {
        locks: &state.nix_eval_locks,
        project_id: project_id.clone(),
    };

    // Load project path, nix_file, and user-configured timeout from DB
    let (project_path, nix_file, timeout_secs) = {
        let db = state.db.lock();
        let project = state
            .projects
            .get(db.conn(), &project_id)
            .map_err(ApiError::Database)?
            .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

        let record = NixService::get(db.conn(), &project_id)
            .map_err(ApiError::Database)?
            .ok_or_else(|| {
                ApiError::InvalidArgument(
                    "No Nix file selected for this project. Call nix_select first.".to_string(),
                )
            })?;

        let general =
            SettingsService::get_general_settings(db.conn()).map_err(ApiError::Database)?;
        // Clamp to a sane range so a bad DB value can't hang the app forever or
        // fire a timeout before nix has even finished spawning.
        let timeout_secs = general.nix_eval_timeout_secs.clamp(30, 3600);

        NixService::set_status(db.conn(), &project_id, NixEnvStatus::Evaluating)
            .map_err(ApiError::Database)?;

        (project.path.clone(), record.nix_file.clone(), timeout_secs)
    };

    // Run evaluation on a blocking thread (can take 30+ seconds on a warm store,
    // many minutes on a cold one).
    let eval_project_path = project_path.clone();
    let eval_nix_file = nix_file.clone();
    let eval_result = tokio::task::spawn_blocking(move || {
        NixService::evaluate(&eval_project_path, &eval_nix_file, timeout_secs)
    })
    .await
    .map_err(|e| ApiError::Internal(format!("Evaluation task panicked: {}", e)))?;

    // Save result to DB and return the updated record in one lock
    let db = state.db.lock();
    match eval_result {
        Ok(env_vars) => {
            NixService::save_success(db.conn(), &project_id, &env_vars)
                .map_err(ApiError::Database)?;
        }
        Err(eval_error) => {
            NixService::save_error(db.conn(), &project_id, &eval_error)
                .map_err(ApiError::Database)?;
            return Err(ApiError::Internal(eval_error.to_string()));
        }
    }

    // _guard drops here, releasing the eval lock

    NixService::get(db.conn(), &project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::Internal("Nix env record disappeared after save".to_string()))
}

/// Clear the Nix environment for a project.
#[tauri::command]
pub fn nix_clear(project_id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    let db = state.db.lock();
    NixService::remove(db.conn(), &project_id).map_err(ApiError::Database)
}

/// Get the current Nix environment status for a project.
#[tauri::command]
pub fn nix_status(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<NixEnvRecord>, ApiError> {
    let db = state.db.lock();
    NixService::get(db.conn(), &project_id).map_err(ApiError::Database)
}

/// Format Nix source code via nixfmt.
#[tauri::command]
pub async fn nix_format(content: String) -> Result<String, ApiError> {
    tokio::task::spawn_blocking(move || NixService::format_nix(&content))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(ApiError::Internal)
}

/// Parse-check a Nix file and return diagnostics.
/// Joins project_path + file_path server-side to avoid frontend path construction.
#[tauri::command]
pub async fn nix_lint(
    project_path: String,
    file_path: String,
) -> Result<Vec<NixDiagnostic>, ApiError> {
    let abs_path = std::path::Path::new(&project_path)
        .join(&file_path)
        .to_string_lossy()
        .to_string();
    tokio::task::spawn_blocking(move || NixService::lint_nix(&abs_path))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(ApiError::Internal)
}

/// Detect providers using the project's Nix-augmented PATH.
#[tauri::command]
pub fn provider_list_for_project(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProviderStatus>, ApiError> {
    let db = state.db.lock();

    let merged_path = match NixService::load_env_vars(db.conn(), &project_id) {
        Ok(Some(nix_env)) => NixService::merged_path(&state.env.merged_path, &nix_env),
        _ => state.env.merged_path.clone(),
    };

    let overrides =
        SettingsService::load_binary_overrides(db.conn()).map_err(ApiError::Database)?;

    let mut providers = state.providers.lock();
    Ok(providers.detect_all(&merged_path, &overrides, Some(&state.env.detected_shell)))
}
