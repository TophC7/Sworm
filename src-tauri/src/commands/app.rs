use crate::app_state::AppState;
use crate::errors::ApiError;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Resolve existing launch paths lexically, preserving symlink path forms.
pub fn launch_path_args(argv: &[String], cwd: Option<&Path>) -> Vec<String> {
    argv.iter()
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .filter_map(|arg| {
            let path = PathBuf::from(arg);
            let path = if path.is_absolute() {
                path
            } else {
                cwd?.join(path)
            };
            let path = crate::services::folders::normalize_absolute_path(&path);
            (path.is_file() || path.is_dir()).then(|| path.to_string_lossy().into_owned())
        })
        .collect()
}

#[derive(Serialize)]
pub struct ClipboardFiles {
    pub op: String,
    pub paths: Vec<String>,
}

#[derive(Serialize)]
pub struct AppRuntimeInfo {
    pub name: String,
    pub version: String,
    pub memory_bytes: Option<u64>,
    pub app_cpu_time_ticks: Option<u64>,
    pub system_cpu_time_ticks: Option<u64>,
    pub thread_count: Option<u32>,
    pub file_descriptor_count: Option<u32>,
}

/// Return package metadata plus a lightweight snapshot of this process.
#[tauri::command]
pub fn app_runtime_info(app: tauri::AppHandle) -> AppRuntimeInfo {
    let package = app.package_info();
    let status = std::fs::read_to_string("/proc/self/status").ok();
    AppRuntimeInfo {
        name: package.name.clone(),
        version: package.version.to_string(),
        memory_bytes: status
            .as_deref()
            .and_then(|value| parse_proc_status_value(value, "VmRSS:"))
            .and_then(|kib| kib.checked_mul(1024)),
        app_cpu_time_ticks: read_process_tree_cpu_time_ticks(std::process::id()),
        system_cpu_time_ticks: std::fs::read_to_string("/proc/stat")
            .ok()
            .and_then(|value| parse_system_cpu_time_ticks(&value)),
        thread_count: status
            .as_deref()
            .and_then(|value| parse_proc_status_value(value, "Threads:"))
            .and_then(|value| u32::try_from(value).ok()),
        file_descriptor_count: std::fs::read_dir("/proc/self/fd")
            .ok()
            .map(|entries| entries.filter_map(Result::ok).count().saturating_sub(1) as u32),
    }
}

#[derive(Debug)]
struct ProcessStat {
    pid: u32,
    parent_pid: u32,
    cpu_time_ticks: u64,
}

fn read_process_tree_cpu_time_ticks(root_pid: u32) -> Option<u64> {
    let processes = std::fs::read_dir("/proc")
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
            let parsed = parse_process_stat(&stat)?;
            (parsed.pid == pid).then_some(parsed)
        })
        .collect::<Vec<_>>();

    process_tree_cpu_time_ticks(root_pid, &processes)
}

fn process_tree_cpu_time_ticks(root_pid: u32, processes: &[ProcessStat]) -> Option<u64> {
    let by_pid = processes
        .iter()
        .map(|process| (process.pid, process))
        .collect::<HashMap<_, _>>();
    if !by_pid.contains_key(&root_pid) {
        return None;
    }

    let mut children = HashMap::<u32, Vec<u32>>::new();
    for process in processes {
        children
            .entry(process.parent_pid)
            .or_default()
            .push(process.pid);
    }

    let mut total = 0u64;
    let mut stack = vec![root_pid];
    let mut visited = HashSet::new();
    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        let process = by_pid.get(&pid)?;
        total = total.checked_add(process.cpu_time_ticks)?;
        if let Some(process_children) = children.get(&pid) {
            stack.extend(process_children);
        }
    }
    Some(total)
}

fn parse_process_stat(stat: &str) -> Option<ProcessStat> {
    let name_start = stat.find('(')?;
    let name_end = stat.rfind(')')?;
    let pid = stat[..name_start].trim().parse::<u32>().ok()?;
    let mut fields = stat[name_end + 1..].split_whitespace();
    let parent_pid = fields.nth(1)?.parse::<u32>().ok()?;
    fields.nth(8)?;
    let cpu_time_ticks = (0..4)
        .try_fold(0i128, |total, _| {
            let value = fields.next()?.parse::<i64>().ok()?;
            total.checked_add(i128::from(value))
        })
        .and_then(|total| u64::try_from(total).ok())?;

    Some(ProcessStat {
        pid,
        parent_pid,
        cpu_time_ticks,
    })
}

fn parse_system_cpu_time_ticks(stat: &str) -> Option<u64> {
    let mut fields = stat
        .lines()
        .find(|line| line.starts_with("cpu "))?
        .split_whitespace()
        .skip(1);
    (0..8).try_fold(0u64, |total, _| {
        total.checked_add(fields.next()?.parse::<u64>().ok()?)
    })
}

fn parse_proc_status_value(status: &str, key: &str) -> Option<u64> {
    status
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse().ok())
}

/// Read a value from the app-state key/value store. Returns `None`
/// when no entry exists for the key.
#[tauri::command]
pub async fn app_state_get(
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    let db = state.db.read();
    state
        .app_state_kv
        .get(db.conn(), &key)
        .map_err(ApiError::Database)
}

/// Write a value to the app-state key/value store.
#[tauri::command]
pub async fn app_state_put(
    key: String,
    value_json: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.write();
    state
        .app_state_kv
        .put(db.conn(), &key, &value_json)
        .map_err(ApiError::Database)
}
/// Delete a value from the app-state key/value store.
#[tauri::command]
pub async fn app_state_delete(
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.write();
    state
        .app_state_kv
        .delete(db.conn(), &key)
        .map_err(ApiError::Database)
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

#[cfg(test)]
mod tests {
    use super::{
        launch_path_args, parse_proc_status_value, parse_process_stat, parse_system_cpu_time_ticks,
        process_tree_cpu_time_ticks, ProcessStat,
    };
    use std::path::Path;

    #[test]
    fn launch_path_args_ignores_argv0_and_flags_and_uses_cwd_for_relative_paths() {
        let root = unique_test_dir("relative-path");
        let cwd = root.join("cwd");
        let project = root.join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&project).unwrap();

        // argv[0] is the binary; leading '-' flags must be skipped.
        let argv = vec!["sworm".into(), "--some-flag".into(), "../project".into()];
        let resolved = launch_path_args(&argv, Some(cwd.as_path()));

        assert_eq!(resolved, vec![project.to_string_lossy().into_owned()]);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn launch_path_args_returns_empty_for_missing_path() {
        let argv = vec!["sworm".into(), "/nonexistent/path/xyz".into()];
        assert!(launch_path_args(&argv, None).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn launch_path_args_preserves_symlink_path_form() {
        let root = unique_test_dir("symlink-path");
        let real = root.join("real-project");
        let link = root.join("project-link");
        std::fs::create_dir_all(&real).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let argv = vec!["sworm".into(), link.to_string_lossy().into_owned()];
        let resolved = launch_path_args(&argv, None);

        assert_eq!(resolved, vec![link.to_string_lossy().into_owned()]);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn parses_proc_status_counts_and_kib_values() {
        let status = "Name:\tsworm\nVmRSS:\t12345 kB\nThreads:\t8\n";

        assert_eq!(parse_proc_status_value(status, "VmRSS:"), Some(12_345));
        assert_eq!(parse_proc_status_value(status, "Threads:"), Some(8));
        assert_eq!(parse_proc_status_value(status, "Missing:"), None);
    }

    #[test]
    fn parses_process_cpu_time_with_spaces_and_parentheses_in_name() {
        let stat = "42 (sworm) worker) S 7 0 0 0 0 0 0 0 0 0 10 20 3 4 0 0 0 0 99\n";

        let process = parse_process_stat(stat).unwrap();

        assert_eq!(process.pid, 42);
        assert_eq!(process.parent_pid, 7);
        assert_eq!(process.cpu_time_ticks, 37);
    }

    #[test]
    fn sums_only_root_process_and_descendants() {
        let processes = [
            ProcessStat {
                pid: 1,
                parent_pid: 0,
                cpu_time_ticks: 10,
            },
            ProcessStat {
                pid: 2,
                parent_pid: 1,
                cpu_time_ticks: 20,
            },
            ProcessStat {
                pid: 3,
                parent_pid: 2,
                cpu_time_ticks: 30,
            },
            ProcessStat {
                pid: 4,
                parent_pid: 0,
                cpu_time_ticks: 100,
            },
        ];

        assert_eq!(process_tree_cpu_time_ticks(1, &processes), Some(60));
        assert_eq!(process_tree_cpu_time_ticks(99, &processes), None);
    }

    #[test]
    fn sums_machine_cpu_capacity_without_double_counting_guest_time() {
        let stat = "cpu  1 2 3 4 5 6 7 8 900 1000\ncpu0 1 2 3 4 5 6 7 8 900 1000\n";

        assert_eq!(parse_system_cpu_time_ticks(stat), Some(36));
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
