//! Pi-specific filesystem layout helpers.
//!
//! Pi has no global session store, so each Sworm session needs its
//! own `--session-dir` under the Tauri app-data directory. These
//! helpers centralize that path so the command layer doesn't grow
//! ad hoc filesystem logic.

use std::path::{Path, PathBuf};

const PI_SESSIONS_DIR: &str = "pi-sessions";

/// Resolve the per-session Pi state directory under `app_data_dir`.
/// The directory is *not* created here; pair with [`ensure_session_dir`]
/// when launching, and with [`remove_session_dir`] on cleanup.
pub fn session_dir(app_data_dir: &Path, session_id: &str) -> PathBuf {
    app_data_dir.join(PI_SESSIONS_DIR).join(session_id)
}

/// Create the per-session Pi state directory if missing. Returns the
/// canonical path so callers can pass it to `pi --session-dir`.
pub fn ensure_session_dir(app_data_dir: &Path, session_id: &str) -> std::io::Result<PathBuf> {
    let dir = session_dir(app_data_dir, session_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Best-effort cleanup of the per-session Pi state directory.
/// Logs but does not propagate filesystem errors — failure here must
/// not block session removal.
pub fn remove_session_dir(app_data_dir: &Path, session_id: &str) {
    let dir = session_dir(app_data_dir, session_id);
    if !dir.exists() {
        return;
    }
    if let Err(error) = std::fs::remove_dir_all(&dir) {
        tracing::warn!(
            "Failed to remove Pi session dir {}: {}",
            dir.display(),
            error
        );
    }
}
