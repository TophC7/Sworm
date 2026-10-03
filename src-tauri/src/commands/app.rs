use crate::app_state::AppState;
use crate::router::Target;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Serialize;
use sworm_core::errors::ApiError;

#[derive(Serialize)]
pub struct ClipboardFiles {
    pub op: String,
    pub paths: Vec<String>,
}

/// Return package metadata, process memory, and aggregate process-tree CPU time.
#[tauri::command]
pub async fn app_runtime_info(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<sworm_protocol::app::AppRuntimeInfo, ApiError> {
    let package = app.package_info();
    state
        .router
        .app_runtime_info(package.name.clone(), package.version.to_string())
        .await
}

/// Everything a URI path may not carry literally. `/` stays a separator, and
/// the RFC 3986 unreserved marks need no escape.
const URI_PATH: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'/')
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn encode_path(path: &str) -> impl std::fmt::Display + '_ {
    utf8_percent_encode(path, URI_PATH)
}

/// Clipboard URI to the path or `sworm://` URI Sworm names files by, or `None`
/// for a scheme naming something no workspace can reach.
///
/// Other applications write RFC 3986, so a name holding a space or `#` arrives
/// percent-encoded and must be decoded before it can be opened.
#[cfg(target_os = "linux")]
fn clipboard_uri_to_path(uri: &str) -> Option<String> {
    if let Some(rest) = uri.strip_prefix("file://") {
        // `file:///p` has an empty authority; `file://localhost/p` names this host.
        let path = rest.strip_prefix("localhost").unwrap_or(rest);
        if !path.starts_with('/') {
            return None;
        }
        return Some(percent_decode_str(path).decode_utf8_lossy().into_owned());
    }
    let remote = uri.strip_prefix("sworm://")?;
    let separator = remote.find('/')?;
    let (server, path) = remote.split_at(separator);
    if server.is_empty() {
        return None;
    }
    let path = percent_decode_str(path).decode_utf8_lossy();
    Some(format!("sworm://{server}{path}"))
}

/// Copy file paths to the system clipboard in file-manager format.
///
/// Writes both `x-special/gnome-copied-files` (Nautilus/Nemo/Caja/Thunar)
/// and `text/uri-list` mimetypes so pasting into a file manager moves
/// or copies the actual files, not text.
///
/// `op` is "copy" or "cut".
#[tauri::command]
pub async fn clipboard_copy_files(paths: Vec<String>, op: String) -> Result<(), ApiError> {
    if paths.is_empty() {
        return Err(ApiError::InvalidArgument("No paths provided".into()));
    }
    if op != "copy" && op != "cut" {
        return Err(ApiError::InvalidArgument(format!("Invalid op: {}", op)));
    }

    let uris = paths
        .iter()
        .map(|path| match Target::parse(path)? {
            Target::Local => Ok(format!("file://{}", encode_path(path))),
            Target::Remote { server, path } => Ok(format!("sworm://{server}{}", encode_path(path))),
        })
        .collect::<Result<Vec<String>, ApiError>>()?;
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
pub async fn clipboard_read_files() -> Result<Option<ClipboardFiles>, ApiError> {
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
                .filter_map(|uri| clipboard_uri_to_path(uri.trim()))
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
        .filter_map(|uri| clipboard_uri_to_path(uri.trim()))
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

#[cfg(test)]
mod tests {
    /// A name holding a space or `#` survives the file clipboard in both
    /// directions: other file managers write RFC 3986, and read their own
    /// spelling back.
    #[cfg(target_os = "linux")]
    #[test]
    fn clipboard_uris_percent_round_trip() {
        use super::{clipboard_uri_to_path, encode_path, parse_gnome_copied_files};

        for path in ["/home/toph/my report #1.txt", "/srv/a b/c%d?e.txt"] {
            let uri = format!("file://{}", encode_path(path));
            assert!(!uri.contains(' '), "{uri}");
            assert_eq!(clipboard_uri_to_path(&uri).as_deref(), Some(path));
        }

        let remote = "sworm://loop/srv/repo/a b.txt";
        assert_eq!(
            clipboard_uri_to_path("sworm://loop/srv/repo/a%20b.txt").as_deref(),
            Some(remote)
        );

        // What Nautilus puts on the clipboard for a spaced name.
        let files = parse_gnome_copied_files(
            "cut\nfile:///home/toph/my%20report%20%231.txt\nsworm://loop/srv/repo/a%20b.txt",
        )
        .expect("two files");
        assert_eq!(files.op, "cut");
        assert_eq!(
            files.paths,
            vec!["/home/toph/my report #1.txt".to_owned(), remote.to_owned()]
        );

        // Schemes no workspace can reach are dropped, not pasted blindly.
        assert_eq!(clipboard_uri_to_path("sftp://host/srv/x"), None);
        assert_eq!(clipboard_uri_to_path("sworm://loop"), None);
    }
}
