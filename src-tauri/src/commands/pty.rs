use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::pty::PtyEvent;

/// Start a demo PTY session running /bin/bash.
///
/// Two channels are passed from the frontend:
/// - `output`: receives raw PTY output bytes
/// - `events`: receives structured lifecycle events
#[tauri::command]
pub fn pty_demo_start(
    session_id: String,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let child_env = state.env.child_env.clone();

    state
        .pty
        .spawn(
            session_id,
            "/bin/bash",
            &[],
            None,
            Some(&child_env),
            cols,
            rows,
            output,
            events,
            true,
            None, // no on_exit callback for demo PTY
        )
        .map_err(|e| ApiError::Pty(e))
}

/// Write data to a demo PTY session.
#[tauri::command]
pub fn pty_demo_write(
    session_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .pty
        .write(&session_id, &data)
        .map_err(|e| ApiError::Pty(e))
}

/// Resize a demo PTY session.
#[tauri::command]
pub fn pty_demo_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .pty
        .resize(&session_id, cols, rows)
        .map_err(|e| ApiError::Pty(e))
}

/// Kill a demo PTY session.
#[tauri::command]
pub fn pty_demo_kill(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.kill(&session_id).map_err(|e| ApiError::Pty(e))
}
