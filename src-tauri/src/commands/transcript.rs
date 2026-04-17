// Commands exposed to the frontend for restoring terminal transcripts.

use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::transcript::TranscriptService;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

/// Return the base64-encoded transcript tail for a session.
///
/// The frontend writes this into xterm on mount so restored tabs open
/// with their saved scrollback. If `limit_bytes` is provided, only the
/// trailing window of that size is returned; otherwise the service
/// default applies (~1 MiB).
#[tauri::command]
pub fn session_transcript_get(
    session_id: String,
    limit_bytes: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let db = state.db.lock();
    let bytes = TranscriptService::read_tail(db.conn(), &session_id, limit_bytes)
        .map_err(ApiError::Database)?;
    Ok(BASE64.encode(bytes))
}

/// Report whether a live PTY currently exists for `session_id` in this
/// app process. Used by the terminal mount flow to decide between
/// historical-only display and live attach.
#[tauri::command]
pub fn session_is_alive(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, ApiError> {
    Ok(state.pty.is_alive(&session_id))
}
