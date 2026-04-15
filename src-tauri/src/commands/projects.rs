use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::project::Project;
use std::path::Path;

/// Open a native directory picker and return the selected path.
#[tauri::command]
pub async fn project_select_directory(app: tauri::AppHandle) -> Result<Option<String>, ApiError> {
    use tauri_plugin_dialog::DialogExt;

    let dir = app.dialog().file().blocking_pick_folder();
    Ok(dir.map(|p| p.to_string()))
}

/// Add a project from a local directory path.
/// Detects branch/base if it's a git repo, but non-repo directories are allowed.
#[tauri::command]
pub fn project_add(path: String, state: tauri::State<'_, AppState>) -> Result<Project, ApiError> {
    let p = Path::new(&path);

    if !p.exists() {
        return Err(ApiError::InvalidArgument(format!(
            "Path does not exist: {}",
            path
        )));
    }

    // Detect git info if available — not required
    let (branch, base_ref) = if state.git.is_git_repo(p) {
        (state.git.current_branch(p), state.git.default_base_ref(p))
    } else {
        (None, None)
    };

    // Derive project name from directory
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    let db = state.db.lock();

    state
        .projects
        .add(
            db.conn(),
            &name,
            &path,
            branch.as_deref(),
            base_ref.as_deref(),
        )
        .map_err(|e| ApiError::Database(e))
}

/// List all projects.
#[tauri::command]
pub fn project_list(state: tauri::State<'_, AppState>) -> Result<Vec<Project>, ApiError> {
    let db = state.db.lock();
    state.projects.list(db.conn()).map_err(ApiError::Database)
}

/// Get a single project by ID.
#[tauri::command]
pub fn project_get(id: String, state: tauri::State<'_, AppState>) -> Result<Project, ApiError> {
    let db = state.db.lock();
    state
        .projects
        .get(db.conn(), &id)
        .map_err(|e| ApiError::Database(e))?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", id)))
}

/// Remove a project by ID.
///
/// Kills all live PTY sessions for the project before deleting
/// DB rows, so no agent processes are orphaned.
#[tauri::command]
pub fn project_remove(id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    let db = state.db.lock();

    // Enumerate the project's sessions so we can kill live PTYs
    let sessions = state
        .sessions
        .list_for_project(db.conn(), &id)
        .unwrap_or_default();

    // Kill any live PTYs for these sessions
    let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    let killed = state.pty.kill_many(&session_ids);
    if killed > 0 {
        tracing::info!("Killed {} live PTY(s) for project {}", killed, id);
    }

    // Now safe to delete -- CASCADE will remove sessions too
    state
        .projects
        .remove(db.conn(), &id)
        .map_err(|e| ApiError::Database(e))
}
