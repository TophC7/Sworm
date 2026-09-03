use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::folder::FolderInfo;
use crate::services::folders::{folder_name, resolve_folder};
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

/// Open a native directory picker and return the selected canonical path.
#[tauri::command]
pub async fn folder_select_directory(app: tauri::AppHandle) -> Result<Option<String>, ApiError> {
    use tauri_plugin_dialog::DialogExt;

    let Some(dir) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    let folder = resolve_folder(&dir.to_string())?;
    Ok(Some(folder.to_string_lossy().into_owned()))
}

/// Canonicalize a folder path and return its display name.
#[tauri::command]
pub async fn folder_resolve(path: String) -> Result<FolderInfo, ApiError> {
    let folder = resolve_folder(&path)?;
    Ok(FolderInfo {
        name: folder_name(&folder),
        path: folder.to_string_lossy().into_owned(),
    })
}

/// List immediate child directories of a canonicalized directory.
#[tauri::command]
pub async fn folder_list_directories(path: String) -> Result<Vec<FolderInfo>, ApiError> {
    list_directories(Path::new(&path))
}

fn list_directories(path: &Path) -> Result<Vec<FolderInfo>, ApiError> {
    let directory = resolve_folder(&path.to_string_lossy())?;
    let mut folders = Vec::new();

    for entry in std::fs::read_dir(directory)? {
        let Ok(entry) = entry else {
            continue;
        };
        let entry_path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() && !(file_type.is_symlink() && entry_path.is_dir()) {
            continue;
        }
        let Ok(canonical_path) = entry_path.canonicalize() else {
            continue;
        };
        folders.push(FolderInfo {
            path: canonical_path.to_string_lossy().into_owned(),
            name: entry.file_name().to_string_lossy().into_owned(),
        });
    }

    folders.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(folders)
}

/// Release backend resources held for a folder once the workbench has
/// closed its last tab: stop settings and file-explorer watches and the
/// issue-bridge socket, then drop the cached issue DB handle and explorer
/// filter. Silent no-op when nothing was held. The workbench only ever
/// hands us paths it got from `folder_resolve`, so when canonicalization
/// fails (folder deleted meanwhile), the raw path is already the resource key.
#[tauri::command]
pub async fn folder_release(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path).unwrap_or_else(|_| PathBuf::from(&folder_path));
    state.issue_bridge.stop(&folder);
    state.issues.evict(&folder);
    state.settings_watchers.stop(&folder);
    state.file_watchers.stop(&folder);
    state.git_watchers.stop(&folder);
    state.files.evict(&folder);
    Ok(())
}

/// Spawn a detached system terminal emulator rooted at the given path.
///
/// Detection order (Linux): $TERMINAL env, then a fallback list of
/// common emulators. The child is fully detached (stdio nulled) so
/// closing Sworm does not nuke the user's shell.
#[tauri::command]
pub async fn folder_open_in_terminal(path: String) -> Result<(), ApiError> {
    let folder = resolve_folder(&path)?;
    spawn_terminal(&folder)
}

#[cfg(target_os = "linux")]
fn spawn_terminal(cwd: &Path) -> Result<(), ApiError> {
    // Setting Command::current_dir isn't enough for emulators that
    // re-exec via dbus activation (gnome-terminal, konsole) or single-
    // instance sockets (kitty, wezterm) -- those drop the spawner's cwd
    // and need an explicit working-directory flag. Build a per-emulator
    // args list keyed off the binary's basename so both $TERMINAL and
    // the fallback list get the correct flag.
    let cwd_str = cwd.to_string_lossy().to_string();
    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();

    if let Ok(term) = std::env::var("TERMINAL") {
        if !term.trim().is_empty() {
            let args = terminal_cwd_args(&term, &cwd_str);
            candidates.push((term, args));
        }
    }

    for name in [
        "x-terminal-emulator",
        "i3-sensible-terminal",
        "wezterm",
        "kitty",
        "alacritty",
        "ghostty",
        "foot",
        "gnome-terminal",
        "konsole",
        "tilix",
        "xfce4-terminal",
        "mate-terminal",
        "lxterminal",
        "terminator",
        "urxvt",
        "st",
        "xterm",
    ] {
        let args = terminal_cwd_args(name, &cwd_str);
        candidates.push((name.to_string(), args));
    }

    let mut last_err: Option<String> = None;
    for (prog, args) in candidates {
        match Command::new(&prog)
            .args(&args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_child) => {
                tracing::info!(
                    "Launched terminal '{}' with args {:?} in {}",
                    prog,
                    args,
                    cwd.display()
                );
                return Ok(());
            }
            Err(e) => {
                last_err = Some(format!("{}: {}", prog, e));
            }
        }
    }

    Err(ApiError::Internal(format!(
        "No terminal emulator found (tried defaults + $TERMINAL). Last error: {}",
        last_err.unwrap_or_else(|| "none".into())
    )))
}

/// Map a terminal emulator's binary name to the argv fragment that
/// forces it to open in `cwd`. Returns an empty list for unknown
/// binaries -- those will still get `Command::current_dir`, which is
/// enough for emulators that don't re-exec (xterm, st, foot...).
#[cfg(target_os = "linux")]
fn terminal_cwd_args(program: &str, cwd: &str) -> Vec<String> {
    let basename = std::path::Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program);

    match basename {
        "kitty" => vec![format!("--directory={}", cwd)],
        "alacritty" => vec!["--working-directory".into(), cwd.into()],
        "wezterm" => vec!["start".into(), "--cwd".into(), cwd.into()],
        "ghostty" => vec![format!("--working-directory={}", cwd)],
        "gnome-terminal" | "tilix" | "mate-terminal" | "xfce4-terminal" | "lxterminal"
        | "terminator" | "foot" => vec![format!("--working-directory={}", cwd)],
        "konsole" => vec!["--workdir".into(), cwd.into()],
        "urxvt" | "rxvt" | "urxvtc" => vec!["-cd".into(), cwd.into()],
        // xterm/st honour the spawner's cwd; x-terminal-emulator is a
        // Debian alternatives symlink, so we don't assume a flag shape.
        _ => vec![],
    }
}

#[cfg(not(target_os = "linux"))]
fn spawn_terminal(_cwd: &Path) -> Result<(), ApiError> {
    Err(ApiError::Internal(
        "Open in terminal is only implemented on Linux".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::list_directories;
    use std::fs;

    #[test]
    fn lists_only_immediate_directories_in_name_order() {
        let root = std::env::temp_dir().join(format!("sworm-folder-list-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("zeta")).unwrap();
        fs::create_dir(root.join("alpha")).unwrap();
        fs::write(root.join("notes.txt"), "not a folder").unwrap();

        let names = list_directories(&root)
            .unwrap()
            .into_iter()
            .map(|folder| folder.name)
            .collect::<Vec<_>>();
        fs::remove_dir_all(root).unwrap();

        assert_eq!(names, ["alpha", "zeta"]);
    }
}
