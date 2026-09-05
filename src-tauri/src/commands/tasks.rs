// Task commands: list, spawn, write, resize, stop.
//
// A "task" is a reusable terminal command defined in
// `<folder>/.sworm/tasks.json`. Each spawn gets a fresh `run_id` used
// as the PTY key — distinct from session IDs so task runs never mix
// with agent sessions in the PTY service's map.

use std::collections::HashMap;

use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::task::TaskDefinition;
use crate::services::folders::resolve_folder;
use crate::services::nix::NixService;
use crate::services::pty::PtyEvent;

/// Return the parsed task list for a folder. Idempotently wires up
/// the file watcher so the frontend receives `tasks-changed` events
/// when `.sworm/tasks.json` is modified externally.
#[tauri::command]
pub async fn tasks_list(
    folder_path: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<TaskDefinition>, ApiError> {
    let folder = resolve_folder(&folder_path)?;

    // Watcher setup is best-effort; a failure here shouldn't block the
    // user from seeing their tasks.
    if let Err(err) = state.tasks.watch(&app, &folder) {
        tracing::warn!(
            "tasks watcher for {} failed to start: {}",
            folder.display(),
            err
        );
    }

    state.tasks.load(&folder).map_err(ApiError::Internal)
}

/// Spawn a PTY running the given task. The frontend generates `run_id`
/// (a UUID) so it can address subsequent write/resize/stop calls. The
/// PTY service accepts any string as its key.
#[tauri::command]
pub async fn tasks_start(
    run_id: String,
    folder_path: String,
    task_id: String,
    active_file_path: Option<String>,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let folder_path = folder.to_string_lossy().into_owned();

    let task = state
        .tasks
        .find(&folder, &task_id)
        .map_err(ApiError::Internal)?
        .ok_or_else(|| ApiError::NotFound(format!("Task not found: {}", task_id)))?;

    // Build the base child env: merge the folder's Nix env over the
    // inherited child env so tasks run in the same shell environment
    // as agent sessions for that folder.
    let base_env = build_task_env(&folder_path, &state);

    let resolved = state
        .tasks
        .resolve(&task, &folder, active_file_path.as_deref(), &base_env);

    // Always shell-wrap so pipes, globs, `&&`, and quoted args work
    // exactly as a user would type them in their own terminal.
    let shell = state.env.detected_shell.clone();
    let shell_args: Vec<&str> = vec!["-c", &resolved.command];

    let cwd_string = resolved.cwd.to_string_lossy().into_owned();

    let on_exit = if task.singleton {
        state
            .tasks
            .register_singleton(folder.clone(), task_id.clone(), run_id.clone())
            .map_err(ApiError::Internal)?;
        let tasks = state.tasks.clone();
        let singleton_folder = folder.clone();
        let singleton_task_id = task_id.clone();
        Some(Box::new(move |_: &str, _: Option<i32>| {
            tasks.release_singleton(&singleton_folder, &singleton_task_id);
        }) as Box<dyn FnOnce(&str, Option<i32>) + Send>)
    } else {
        None
    };

    let spawn_result = state.pty.spawn(
        run_id.clone(),
        &shell,
        &shell_args,
        Some(&cwd_string),
        Some(&resolved.env),
        cols,
        rows,
        output,
        events,
        Some(window.label().to_string()),
        on_exit,
    );
    if let Err(error) = spawn_result {
        state.tasks.release_singleton_by_run_id(&run_id);
        return Err(ApiError::Pty(error));
    }
    Ok(())
}

#[tauri::command]
pub async fn tasks_write(
    run_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.write(&run_id, &data).map_err(ApiError::Pty)
}

#[tauri::command]
pub async fn tasks_resize(
    run_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.resize(&run_id, cols, rows).map_err(ApiError::Pty)
}

#[tauri::command]
pub async fn tasks_stop(run_id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    // Kill is a no-op-in-effect if the PTY already exited; swallow
    // the "no active PTY session" case so the frontend can call stop
    // on an already-exited tab without seeing a spurious error.
    let result = match state.pty.kill(&run_id) {
        Ok(()) => Ok(()),
        Err(err) if err.contains("No active PTY session") => Ok(()),
        Err(err) => Err(ApiError::Pty(err)),
    };
    state.tasks.release_singleton_by_run_id(&run_id);
    result
}

fn build_task_env(
    folder_path: &str,
    state: &tauri::State<'_, AppState>,
) -> HashMap<String, String> {
    let nix_env = {
        let db = state.db.read();
        NixService::load_env_vars(db.conn(), folder_path).unwrap_or_default()
    };

    match nix_env {
        Some(nix) => NixService::merge_env(&state.env.child_env, &nix),
        None => state.env.child_env.clone(),
    }
}
