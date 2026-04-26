use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::env::EnvProbeResult;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// Slot for a directory path passed on argv (e.g. from Nautilus
/// "Open With"). Filled at startup or by the single-instance
/// callback; the frontend drains it via `app_take_pending_open_path`
/// on mount / when the `sworm://pending-open-changed` event fires.
#[derive(Default)]
pub struct PendingOpen(pub Mutex<Option<String>>);

/// Pull the first argv entry that looks like an existing directory.
/// Skips flag-style args so webview/Tauri internals don't get
/// misinterpreted as paths. Relative paths are resolved against the
/// caller's cwd, then normalized lexically so `.`/`..` components
/// don't create duplicate project rows while symlink path forms are
/// preserved.
pub fn first_dir_arg(argv: &[String], cwd: Option<&Path>) -> Option<String> {
    argv.iter().skip(1).find_map(|raw| {
        if raw.starts_with('-') {
            return None;
        }
        let raw_path = Path::new(raw);
        let candidate = absolutize_dir_arg(raw_path, cwd)?;
        if !candidate.is_dir() {
            return None;
        }
        Some(
            normalize_absolute_path(&candidate)
                .to_string_lossy()
                .into_owned(),
        )
    })
}

fn absolutize_dir_arg(path: &Path, cwd: Option<&Path>) -> Option<PathBuf> {
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    cwd.map(|base| base.join(path))
}

fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(
                    normalized.components().next_back(),
                    Some(Component::Normal(_))
                );
                if can_pop {
                    normalized.pop();
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }

    normalized
}

#[derive(Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct ClipboardFiles {
    pub op: String,
    pub paths: Vec<String>,
}

/// Trivial health check to prove the IPC bridge works.
#[tauri::command]
pub fn health_ping() -> String {
    "pong".to_string()
}

/// Return basic app metadata.
#[tauri::command]
pub fn app_get_info() -> AppInfo {
    AppInfo {
        name: "Sworm".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Database smoke test: open DB, run migrations, verify a query works.
#[tauri::command]
pub fn db_smoke_test(state: tauri::State<'_, AppState>) -> Result<String, ApiError> {
    state
        .db
        .smoke_test()
        .map_err(|e| ApiError::Database(e.to_string()))
}

/// Keyring smoke test: write/read/delete a test secret.
#[tauri::command]
pub fn keyring_smoke_test(state: tauri::State<'_, AppState>) -> Result<String, ApiError> {
    state
        .credentials
        .smoke_test()
        .map_err(|e| ApiError::Keyring(e))
}

/// Return environment probe diagnostics.
#[tauri::command]
pub fn env_probe(state: tauri::State<'_, AppState>) -> EnvProbeResult {
    state.env.probe_result()
}

/// Copy file paths to the system clipboard in file-manager format.
///
/// Writes both `x-special/gnome-copied-files` (Nautilus/Nemo/Caja/Thunar)
/// and `text/uri-list` mimetypes so pasting into a file manager moves
/// or copies the actual files, not text.
///
/// `op` is "copy" or "cut".
#[tauri::command]
pub fn clipboard_copy_files(paths: Vec<String>, op: String) -> Result<(), ApiError> {
    if paths.is_empty() {
        return Err(ApiError::InvalidArgument("No paths provided".into()));
    }
    if op != "copy" && op != "cut" {
        return Err(ApiError::InvalidArgument(format!("Invalid op: {}", op)));
    }

    let uris: Vec<String> = paths.iter().map(|p| format!("file://{}", p)).collect();
    // GNOME/Nautilus format; verified against Nautilus 49.
    // Format: "op\nuri1\nuri2"; NO trailing newline.
    let gnome_data = format!("{}\n{}", op, uris.join("\n"));
    // Drag-and-drop compat; WITH trailing newline per RFC 2483 + Nautilus.
    let uri_list = format!("{}\n", uris.join("\n"));

    copy_files_wayland(&gnome_data, &uri_list)
}

#[cfg(target_os = "linux")]
fn copy_files_wayland(gnome_data: &str, uri_list: &str) -> Result<(), ApiError> {
    use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

    let sources = vec![
        MimeSource {
            source: Source::Bytes(gnome_data.as_bytes().to_vec().into_boxed_slice()),
            mime_type: MimeType::Specific("x-special/gnome-copied-files".into()),
        },
        MimeSource {
            source: Source::Bytes(uri_list.as_bytes().to_vec().into_boxed_slice()),
            mime_type: MimeType::Specific("text/uri-list".into()),
        },
    ];

    Options::new()
        .copy_multi(sources)
        .map_err(|e| ApiError::Internal(format!("wl-clipboard copy failed: {}", e)))
}

#[cfg(not(target_os = "linux"))]
fn copy_files_wayland(_gnome_data: &str, _uri_list: &str) -> Result<(), ApiError> {
    Err(ApiError::Internal(
        "File clipboard not implemented on this platform".into(),
    ))
}

/// Read file URIs + op (copy/cut) from the system clipboard.
///
/// Returns `None` if the clipboard doesn't contain a recognizable file list.
#[tauri::command]
pub fn clipboard_read_files() -> Result<Option<ClipboardFiles>, ApiError> {
    read_clipboard_files()
}

#[cfg(target_os = "linux")]
fn read_clipboard_files() -> Result<Option<ClipboardFiles>, ApiError> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{
        get_contents, ClipboardType, Error as PasteError, MimeType, Seat,
    };

    // Try x-special/gnome-copied-files first; it has op + uris.
    let gnome = get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific("x-special/gnome-copied-files"),
    );
    match gnome {
        Ok((mut reader, _mime)) => {
            let mut body = String::new();
            reader
                .read_to_string(&mut body)
                .map_err(|e| ApiError::Internal(format!("clipboard read failed: {}", e)))?;
            if let Some(files) = parse_gnome_copied_files(&body) {
                return Ok(Some(files));
            }
        }
        Err(PasteError::NoMimeType) | Err(PasteError::ClipboardEmpty) => {}
        Err(e) => return Err(ApiError::Internal(format!("clipboard read failed: {}", e))),
    }

    // Fallback: text/uri-list; treat as copy.
    let uri_list = get_contents(
        ClipboardType::Regular,
        Seat::Unspecified,
        MimeType::Specific("text/uri-list"),
    );
    match uri_list {
        Ok((mut reader, _mime)) => {
            let mut body = String::new();
            reader
                .read_to_string(&mut body)
                .map_err(|e| ApiError::Internal(format!("clipboard read failed: {}", e)))?;
            let paths: Vec<String> = body
                .lines()
                .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
                .filter_map(|uri| uri.strip_prefix("file://").map(|p| p.to_string()))
                .collect();
            if !paths.is_empty() {
                return Ok(Some(ClipboardFiles {
                    op: "copy".into(),
                    paths,
                }));
            }
        }
        Err(PasteError::NoMimeType) | Err(PasteError::ClipboardEmpty) => {}
        Err(e) => return Err(ApiError::Internal(format!("clipboard read failed: {}", e))),
    }

    Ok(None)
}

#[cfg(target_os = "linux")]
fn parse_gnome_copied_files(body: &str) -> Option<ClipboardFiles> {
    let mut lines = body.lines();
    let op = lines.next()?;
    if op != "copy" && op != "cut" {
        return None;
    }
    let paths: Vec<String> = lines
        .filter(|l| !l.trim().is_empty())
        .filter_map(|uri| uri.strip_prefix("file://").map(|p| p.to_string()))
        .collect();
    if paths.is_empty() {
        return None;
    }
    Some(ClipboardFiles {
        op: op.to_string(),
        paths,
    })
}

#[cfg(not(target_os = "linux"))]
fn read_clipboard_files() -> Result<Option<ClipboardFiles>, ApiError> {
    Err(ApiError::Internal(
        "File clipboard not implemented on this platform".into(),
    ))
}

/// Drain the pending-open-path slot (populated by argv at launch or
/// by the single-instance callback on subsequent launches).
/// Returns None when there's nothing queued; callers should ignore.
#[tauri::command]
pub fn app_take_pending_open_path(pending: tauri::State<'_, PendingOpen>) -> Option<String> {
    pending.0.lock().ok().and_then(|mut slot| slot.take())
}

#[cfg(test)]
mod tests {
    use super::first_dir_arg;
    use std::path::Path;

    #[test]
    fn first_dir_arg_ignores_argv0_and_flags_and_uses_cwd_for_relative_paths() {
        let root = unique_test_dir("relative-path");
        let cwd = root.join("cwd");
        let project = root.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        // argv[0] is the binary; leading '-' flags must be skipped.
        let argv = vec!["sworm".into(), "--some-flag".into(), "../project".into()];
        let resolved = first_dir_arg(&argv, Some(cwd.as_path()));

        assert_eq!(
            resolved.as_deref(),
            Some(project.to_string_lossy().as_ref())
        );

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn first_dir_arg_returns_none_for_missing_path() {
        let argv = vec!["sworm".into(), "/nonexistent/path/xyz".into()];
        assert!(first_dir_arg(&argv, None).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn first_dir_arg_preserves_symlink_path_form() {
        let root = unique_test_dir("symlink-path");
        let real = root.join("real-project");
        let link = root.join("project-link");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let argv = vec!["sworm".into(), link.to_string_lossy().into_owned()];
        let resolved = first_dir_arg(&argv, None);

        assert_eq!(resolved.as_deref(), Some(link.to_string_lossy().as_ref()));

        std::fs::remove_dir_all(&root).unwrap();
    }

    fn unique_test_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sworm-app-command-test-{}-{}",
            label,
            uuid::Uuid::new_v4()
        ));
        if Path::new(&dir).exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        dir
    }
}
