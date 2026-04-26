// Task commands: list, spawn, write, resize, stop.
//
// A "task" is a reusable terminal command defined in
// `<project>/.sworm/tasks.json`. Each spawn gets a fresh `run_id` used
// as the PTY key — distinct from session IDs so task runs never mix
// with agent sessions in the PTY service's map.

use std::collections::HashMap;

use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::task::TaskDefinition;
use crate::services::nix::NixService;
use crate::services::pty::PtyEvent;

/// Return the parsed task list for a project. Idempotently wires up
/// the file watcher so the frontend receives `tasks-changed` events
/// when `.sworm/tasks.json` is modified externally.
#[tauri::command]
pub fn tasks_list(
    project_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TaskDefinition>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;

    // Watcher setup is best-effort; a failure here shouldn't block the
    // user from seeing their tasks.
    if let Err(err) = state.tasks.watch(&app, &project_id, &project_path) {
        tracing::warn!("tasks watcher for {} failed to start: {}", project_id, err);
    }

    state.tasks.load(&project_path).map_err(ApiError::Internal)
}

/// Spawn a PTY running the given task. The frontend generates `run_id`
/// (a UUID) so it can address subsequent write/resize/stop calls. The
/// PTY service accepts any string as its key.
#[tauri::command]
pub fn tasks_start(
    run_id: String,
    project_id: String,
    task_id: String,
    active_file_path: Option<String>,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project_path = project_path_for(&project_id, &state)?;

    let task = state
        .tasks
        .find(&project_path, &task_id)
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("Task not found: {}", task_id)))?;

    // Build the base child env: merge the project's Nix env over the
    // inherited child env so tasks run in the same shell environment
    // as agent sessions for that project.
    let base_env = build_task_env(&project_id, &state);

    let resolved =
        state
            .tasks
            .resolve(&task, &project_path, active_file_path.as_deref(), &base_env);

    // Always shell-wrap so pipes, globs, `&&`, and quoted args work
    // exactly as a user would type them in their own terminal.
    let shell = state.env.detected_shell.clone();
    let shell_args: Vec<&str> = vec!["-c", &resolved.command];

    let cwd_string = resolved.cwd.to_string_lossy().into_owned();

    state
        .pty
        .spawn(
            run_id,
            &shell,
            &shell_args,
            Some(&cwd_string),
            Some(&resolved.env),
            cols,
            rows,
            output,
            events,
            false,
            None,
        )
        .map_err(ApiError::Pty)
}

#[tauri::command]
pub fn tasks_write(
    run_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.write(&run_id, &data).map_err(ApiError::Pty)
}

#[tauri::command]
pub fn tasks_resize(
    run_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.resize(&run_id, cols, rows).map_err(ApiError::Pty)
}

#[tauri::command]
pub fn tasks_stop(run_id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    // Kill is a no-op-in-effect if the PTY already exited; swallow
    // the "no active PTY session" case so the frontend can call stop
    // on an already-exited tab without seeing a spurious error.
    match state.pty.kill(&run_id) {
        Ok(()) => Ok(()),
        Err(err) if err.contains("No active PTY session") => Ok(()),
        Err(err) => Err(ApiError::Pty(err)),
    }
}

fn project_path_for(
    project_id: &str,
    state: &tauri::State<'_, AppState>,
) -> Result<std::path::PathBuf, ApiError> {
    let db = state.db.read();
    let project = state
        .projects
        .get(db.conn(), project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;
    Ok(std::path::PathBuf::from(project.path))
}

fn build_task_env(project_id: &str, state: &tauri::State<'_, AppState>) -> HashMap<String, String> {
    let nix_env = {
        let db = state.db.read();
        NixService::load_env_vars(db.conn(), project_id).unwrap_or_default()
    };

    match nix_env {
        Some(nix) => NixService::merge_env(&state.env.child_env, &nix),
        None => state.env.child_env.clone(),
    }
}
