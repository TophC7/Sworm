mod app_state;
mod commands;
mod deep_links;
pub mod host_events;
pub mod router;
mod windows;

use crate::deep_links::{deep_link_arg, launch_path_args};
use app_state::AppState;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;
use windows::{focus_window, WindowCoordinatorService};

pub fn run() {
    configure_linux_gdk_backend();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sworm_lib=info,sworm_core=info".into()),
        )
        .init();

    tracing::info!("Sworm starting up");

    let app = tauri::Builder::default()
        // Keep this first: it forwards URL launches to the deep-link plugin,
        // which routes them before this callback runs.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let state = app.state::<AppState>();
            if let Some(url) = deep_link_arg(&argv) {
                // Reuse the last active workbench; pairing must not spawn a
                // blank window or interfere with normal multi-window launches.
                // Workspace links already picked their window via the
                // folder-open setting; refocusing here would undo that.
                if url.scheme() != "sworm-pair" {
                    return;
                }
                if let Some(window) = state
                    .windows
                    .get_focused_window_label()
                    .and_then(|label| app.get_webview_window(&label))
                {
                    focus_window(app, window.label());
                } else if let Err(error) = state.windows.create_workbench_window(app, None) {
                    tracing::error!("Failed to create window for deep link: {error}");
                }
                return;
            }
            let paths = launch_path_args(&argv, Some(Path::new(&cwd)));
            if paths.is_empty() {
                if let Err(error) = state.windows.create_workbench_window(app, None) {
                    tracing::error!("Failed to create window for second launch: {error}");
                }
            } else {
                for path in paths {
                    state.windows.route_open_path(app, &path);
                }
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(deep_links::DeepLinks::default())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            let windows = Arc::clone(&state.windows);
            let manifest = {
                let db = state.host.db.write();
                WindowCoordinatorService::load_manifest(db.conn()).map_err(std::io::Error::other)?
            };
            state.router.retry_pending_stops();
            app.manage(state);

            for entry in manifest.windows {
                windows
                    .create_workbench_window(app.handle(), Some(entry))
                    .map_err(std::io::Error::other)?;
            }

            let argv: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().ok();
            // The deep-link plugin owns lone URL launches; argv routing here
            // would open the same workspace twice.
            if deep_link_arg(&argv).is_none() {
                for path in launch_path_args(&argv, cwd.as_deref()) {
                    windows.route_open_path(app.handle(), &path);
                    tracing::info!("First-launch argv opened path: {path}");
                }
            }
            // Before the blank-window fallback: a workspace link opens its own.
            deep_links::init(app.handle()).map_err(std::io::Error::other)?;

            // Routing creates windows for argv targets; only fall back to a
            // blank window when neither restore nor argv produced one.
            if windows.window_count() == 0 {
                windows
                    .create_workbench_window(app.handle(), None)
                    .map_err(std::io::Error::other)?;
            }

            tracing::info!("AppState initialized");

            Ok(())
        })
        .invoke_handler(commands::invoke_handler())
        .build(tauri::generate_context!())
        .expect("error building Sworm");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } => {
            let state = app_handle.state::<AppState>();
            state.windows.request_exit();
            state.windows.wait_for_cleanup();
            if let Err(error) = state.windows.save_manifest(app_handle) {
                tracing::error!("Failed to save window manifest on exit: {error}");
            }
        }
        tauri::RunEvent::Exit => {
            let state = app_handle.state::<AppState>();
            state.windows.wait_for_cleanup();
            let (cleaned, lsp_cleaned) = state.host.shutdown();
            tracing::info!(
                "App exit cleanup finished, killed {} PTY sessions and {} LSP sessions",
                cleaned,
                lsp_cleaned
            );
        }
        _ => {}
    });
}

/// Prefer native Wayland while preserving an explicit backend override.
///
/// Called before the Tauri runtime (and its threads) starts.
/// `std::env::set_var` is safe here because no other threads exist yet;
/// it will become `unsafe` in Rust 2024 edition; revisit when upgrading.
#[cfg(target_os = "linux")]
fn configure_linux_gdk_backend() {
    if std::env::var_os("GDK_BACKEND").is_none_or(|value| value.is_empty()) {
        std::env::set_var("GDK_BACKEND", "wayland,x11");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_gdk_backend() {}
