//! OMP filesystem layout helpers.
//!
//! Sworm isolates each session from OMP's global store with a private
//! `--session-dir` under the Tauri app-data directory. These helpers
//! centralize that path so the command layer doesn't grow ad hoc
//! filesystem logic.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

const OMP_SESSIONS_DIR: &str = "omp-sessions";

/// Resolve the per-session OMP state directory under `app_data_dir`.
/// The directory is *not* created here; pair with [`ensure_session_dir`]
/// when launching, and with [`remove_session_dir`] on cleanup.
pub fn session_dir(app_data_dir: &Path, session_id: &str) -> PathBuf {
    app_data_dir.join(OMP_SESSIONS_DIR).join(session_id)
}

/// Create the per-session OMP state directory if missing. Returns the
/// canonical path so callers can pass it to `omp --session-dir`.
pub fn ensure_session_dir(app_data_dir: &Path, session_id: &str) -> std::io::Result<PathBuf> {
    let dir = session_dir(app_data_dir, session_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Whether a session directory already holds OMP state, i.e. the session
/// can be resumed with `--continue`.
pub fn session_dir_has_state(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

/// Best-effort cleanup of the per-session OMP state directory.
/// Logs but does not propagate filesystem errors — failure here must
/// not block session removal.
pub fn remove_session_dir(app_data_dir: &Path, session_id: &str) {
    let dir = session_dir(app_data_dir, session_id);
    if let Err(error) = std::fs::remove_dir_all(&dir) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(
                "Failed to remove OMP session dir {}: {}",
                dir.display(),
                error
            );
        }
    }
}

/// Remove every per-session OMP state directory whose session id is not
/// in `keep`. Runs once at startup after the workbench restores its tabs.
pub fn remove_session_dirs_except(app_data_dir: &Path, keep: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(app_data_dir.join(OMP_SESSIONS_DIR)) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(session_id) = name.to_str() else {
            continue;
        };
        if !keep.contains(session_id) {
            remove_session_dir(app_data_dir, session_id);
        }
    }
}
