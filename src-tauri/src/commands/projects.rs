use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::project::Project;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

/// Open a native directory picker and return the selected path.
#[tauri::command]
pub async fn project_select_directory(app: tauri::AppHandle) -> Result<Option<String>, ApiError> {
    use tauri_plugin_dialog::DialogExt;

    let dir = app.dialog().file().blocking_pick_folder();
    Ok(dir.map(|p| p.to_string()))
}

/// Add a project from a local directory path.
/// Detects branch/base if it's a git repo, but non-repo directories are allowed.
#[tauri::command]
pub fn project_add(path: String, state: tauri::State<'_, AppState>) -> Result<Project, ApiError> {
    let p = Path::new(&path);

    if !p.exists() {
        return Err(ApiError::InvalidArgument(format!(
            "Path does not exist: {}",
            path
        )));
    }
    if !p.is_dir() {
        return Err(ApiError::InvalidArgument(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    let db = state.db.lock();
    if let Some(existing) = state
        .projects
        .list(db.conn())
        .map_err(ApiError::Database)?
        .into_iter()
        .find(|project| same_project_directory(project.path.as_str(), p))
    {
        return Ok(existing);
    }

    // Detect git info if available — not required
    let (branch, base_ref) = if state.git.is_git_repo(p) {
        (state.git.current_branch(p), state.git.default_base_ref(p))
    } else {
        (None, None)
    };

    // Derive project name from directory
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    state
        .projects
        .add(
            db.conn(),
            &name,
            &path,
            branch.as_deref(),
            base_ref.as_deref(),
        )
        .map_err(|e| ApiError::Database(e))
}

fn same_project_directory(existing_path: &str, requested: &Path) -> bool {
    if Path::new(existing_path) == requested {
        return true;
    }

    let Ok(requested_canonical) = requested.canonicalize() else {
        return false;
    };
    let Ok(existing_canonical) = Path::new(existing_path).canonicalize() else {
        return false;
    };

    existing_canonical == requested_canonical
}

/// List all projects.
#[tauri::command]
pub fn project_list(state: tauri::State<'_, AppState>) -> Result<Vec<Project>, ApiError> {
    let db = state.db.lock();
    state.projects.list(db.conn()).map_err(ApiError::Database)
}

/// Get a single project by ID.
#[tauri::command]
pub fn project_get(id: String, state: tauri::State<'_, AppState>) -> Result<Project, ApiError> {
    let db = state.db.lock();
    state
        .projects
        .get(db.conn(), &id)
        .map_err(|e| ApiError::Database(e))?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", id)))
}

/// Remove a project by ID.
///
/// Kills all live PTY sessions for the project before deleting
/// DB rows, so no agent processes are orphaned.
#[tauri::command]
pub fn project_remove(id: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    let db = state.db.lock();

    // Enumerate the project's sessions so we can kill live PTYs
    let sessions = state
        .sessions
        .list_for_project(db.conn(), &id)
        .unwrap_or_default();

    // Kill any live PTYs for these sessions
    let session_ids: Vec<String> = sessions.iter().map(|s| s.id.clone()).collect();
    let killed = state.pty.kill_many(&session_ids);
    if killed > 0 {
        tracing::info!("Killed {} live PTY(s) for project {}", killed, id);
    }

    // Now safe to delete -- CASCADE will remove sessions too
    state
        .projects
        .remove(db.conn(), &id)
        .map_err(|e| ApiError::Database(e))
}

/// Spawn a detached system terminal emulator rooted at the given path.
///
/// Detection order (Linux): $TERMINAL env, then a fallback list of
/// common emulators. The child is fully detached (stdio nulled) so
/// closing Sworm does not nuke the user's shell.
#[tauri::command]
pub fn project_open_in_terminal(path: String) -> Result<(), ApiError> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(ApiError::InvalidArgument(format!(
            "Path does not exist: {}",
            path
        )));
    }
    if !p.is_dir() {
        return Err(ApiError::InvalidArgument(format!(
            "Path is not a directory: {}",
            path
        )));
    }

    spawn_terminal(p)
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
