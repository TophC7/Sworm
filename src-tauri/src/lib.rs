mod app_state;
mod commands;
mod errors;
mod models;
mod services;

use app_state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ade_lib=info".into()),
        )
        .init();

    tracing::info!("ADE starting up");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            app.manage(state);
            tracing::info!("AppState initialized");

            // GTK always draws client-side decorations (CSD) and ignores the
            // Wayland xdg-decoration protocol. Detect tiling compositors and
            // strip decorations so the WM's own chrome (or none) takes over.
            if should_disable_decorations() {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_decorations(false);
                    tracing::info!("Disabled CSD for tiling compositor");
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // App commands
            commands::app::health_ping,
            commands::app::app_get_info,
            commands::app::db_smoke_test,
            commands::app::keyring_smoke_test,
            commands::app::env_probe,
            // Project commands
            commands::projects::project_select_directory,
            commands::projects::project_add,
            commands::projects::project_list,
            commands::projects::project_get,
            commands::projects::project_remove,
            // Provider commands
            commands::providers::provider_list,
            commands::providers::provider_refresh,
            // Settings commands
            commands::settings::settings_get,
            commands::settings::settings_set_general,
            commands::settings::settings_set_provider_config,
            // Session commands
            commands::sessions::session_create,
            commands::sessions::session_list,
            commands::sessions::session_get,
            commands::sessions::session_start,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_stop,
            commands::sessions::session_reset,
            commands::sessions::session_remove,
            // Git commands
            commands::git::git_get_summary,
            commands::git::git_get_file_diff,
            commands::git::git_get_log,
            // PTY demo commands (kept for backwards compat)
            commands::pty::pty_demo_start,
            commands::pty::pty_demo_write,
            commands::pty::pty_demo_resize,
            commands::pty::pty_demo_kill,
        ])
        .build(tauri::generate_context!())
        .expect("error building ADE");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let cleaned = app_handle.state::<AppState>().pty.kill_all();
            tracing::info!("App exit cleanup finished, killed {} PTY sessions", cleaned);
        }
    });
}

/// Check whether the desktop environment is a tiling compositor
/// that manages its own window chrome (or uses none at all).
///
/// Checks `ADE_DECORATIONS` env var first (explicit override),
/// then falls back to detecting known tiling Wayland compositors
/// via `XDG_CURRENT_DESKTOP`.
fn should_disable_decorations() -> bool {
    // Explicit override: ADE_DECORATIONS=0 forces off, =1 forces on
    if let Ok(val) = std::env::var("ADE_DECORATIONS") {
        return val == "0";
    }

    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase();

    // Known tiling Wayland compositors that don't want CSD
    const TILING_DESKTOPS: &[&str] = &[
        "niri",
        "sway",
        "hyprland",
        "river",
        "dwl",
        "qtile",
        "i3",
    ];

    TILING_DESKTOPS.iter().any(|&wm| desktop.contains(wm))
}
