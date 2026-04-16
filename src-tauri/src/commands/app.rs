use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::services::env::EnvProbeResult;
use serde::Serialize;

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
    let db = state.db.lock();
    db.smoke_test()
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
    // GNOME/Nautilus format — verified against Nautilus 49.
    // Format: "op\nuri1\nuri2" — NO trailing newline.
    let gnome_data = format!("{}\n{}", op, uris.join("\n"));
    // Drag-and-drop compat — WITH trailing newline per RFC 2483 + Nautilus.
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

    // Try x-special/gnome-copied-files first — it has op + uris.
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

    // Fallback: text/uri-list — treat as copy.
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
