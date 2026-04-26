mod app_state;
mod commands;
mod errors;
mod models;
mod services;

use app_state::AppState;
use commands::app::{first_dir_arg, PendingOpen};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_linux_runtime_env();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sworm_lib=info".into()),
        )
        .init();

    tracing::info!("Sworm starting up");

    let app = tauri::Builder::default()
        // Single-instance lock: second launch focuses the existing window
        // instead of spinning up a parallel process. The plugin keys its
        // lock off the bundle identifier, so dev and prod builds (which
        // use different identifiers) each get their own lock; allowing
        // dogfooding Sworm-in-Sworm without the two instances fighting
        // over the same SQLite DB.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            // Subsequent-launch path: Nautilus "Open With" on an already-
            // running Sworm re-invokes the binary, lands here. Stash the
            // path and wake the frontend so it can drain it via the
            // app_take_pending_open_path command.
            if let Some(path) = first_dir_arg(&argv, Some(std::path::Path::new(&cwd))) {
                if let Some(pending) = app.try_state::<PendingOpen>() {
                    if let Ok(mut slot) = pending.0.lock() {
                        *slot = Some(path.clone());
                    }
                }
                let _ = app.emit("sworm://pending-open-changed", ());
                tracing::info!("Second-instance argv opened path: {}", path);
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            app.manage(state);

            // First-launch argv capture: cold-start from Nautilus
            // passes the folder as argv[1]. Park it for the frontend
            // to drain in onMount. The single-instance callback above
            // handles the warm-start case.
            let pending = PendingOpen::default();
            let argv: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().ok();
            if let Some(path) = first_dir_arg(&argv, cwd.as_deref()) {
                if let Ok(mut slot) = pending.0.lock() {
                    *slot = Some(path.clone());
                }
                tracing::info!("First-launch argv opened path: {}", path);
            }
            app.manage(pending);

            tracing::info!("AppState initialized");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Activity map commands
            commands::activity_map::activity_map_get,
            commands::activity_map::activity_map_refresh,
            // App commands
            commands::app::health_ping,
            commands::app::app_get_info,
            commands::app::db_smoke_test,
            commands::app::keyring_smoke_test,
            commands::app::env_probe,
            commands::app::clipboard_copy_files,
            commands::app::clipboard_read_files,
            commands::app::app_take_pending_open_path,
            // Builtins commands
            commands::builtins::builtins_get_catalog,
            // Config schema commands (drives Monaco autocomplete for .sworm/*.json)
            commands::config_schemas::config_schemas_list,
            // Drag and drop commands
            commands::dnd::dnd_save_dropped_bytes,
            // Project commands
            commands::projects::project_select_directory,
            commands::projects::project_add,
            commands::projects::project_refresh_git,
            commands::projects::project_list,
            commands::projects::project_get,
            commands::projects::project_remove,
            commands::projects::project_open_in_terminal,
            // Provider commands
            commands::providers::provider_list,
            commands::providers::provider_refresh,
            // Settings commands
            commands::settings::settings_get,
            commands::settings::settings_set_general,
            commands::settings::settings_set_formatting,
            commands::settings::settings_set_provider_config,
            // Formatter commands
            commands::formatting::formatting_format_biome,
            commands::formatting::formatting_format_nixfmt,
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
            commands::sessions::session_archive,
            commands::sessions::session_unarchive,
            commands::sessions::session_list_archived,
            // Task commands (project-scoped .sworm/tasks.json)
            commands::tasks::tasks_list,
            commands::tasks::tasks_start,
            commands::tasks::tasks_write,
            commands::tasks::tasks_resize,
            commands::tasks::tasks_stop,
            // Transcript / liveness commands (recovery)
            commands::transcript::session_transcript_get,
            commands::transcript::session_is_alive,
            // Workspace persistence commands
            commands::workspace::workspace_state_get,
            commands::workspace::workspace_state_put,
            commands::workspace::app_state_get,
            commands::workspace::app_state_put,
            // Nix environment commands
            commands::nix::nix_detect,
            commands::nix::nix_select,
            commands::nix::nix_evaluate,
            commands::nix::nix_clear,
            commands::nix::nix_status,
            commands::nix::nix_format,
            commands::nix::nix_lint,
            commands::nix::provider_list_for_project,
            // Fresh editor commands
            commands::fresh::editor_open_file,
            commands::fresh::editor_open_at_commit,
            commands::fresh::editor_open_at_stash,
            // File commands
            commands::files::file_read,
            commands::files::file_write,
            commands::files::file_create_dir,
            commands::files::file_rename,
            commands::files::file_stat,
            commands::files::file_delete,
            commands::files::file_paste,
            commands::files::file_paste_collisions,
            commands::files::files_list_all,
            // Git commands
            commands::git::git_get_summary,
            commands::git::git_get_graph,
            commands::git::git_get_commit_detail,
            commands::git::diff_get_files,
            commands::git::diff_get_working_index,
            commands::git::diff_get_working_file,
            // Git write commands
            commands::git::git_stage_all,
            commands::git::git_stage_files,
            commands::git::git_unstage_all,
            commands::git::git_unstage_files,
            commands::git::git_discard_all,
            commands::git::git_discard_files,
            commands::git::git_get_full_patch,
            commands::git::git_get_path_patch,
            commands::git::git_get_quick_diff_data,
            commands::git::git_stage_file_content,
            commands::git::git_commit,
            commands::git::git_undo_last_commit,
            commands::git::git_push,
            commands::git::git_push_force_with_lease,
            commands::git::git_pull,
            commands::git::git_fetch,
            commands::git::git_stash_all,
            commands::git::git_stash_count,
            commands::git::git_stash_list,
            commands::git::git_stash_pop,
            commands::git::git_stash_drop,
            commands::git::git_show_file,
            commands::git::git_init,
            commands::git::git_clone_in_place,
            // LSP commands
            commands::lsp::lsp_list_servers,
            commands::lsp::lsp_set_server_config,
            commands::lsp::lsp_start,
            commands::lsp::lsp_send,
            commands::lsp::lsp_stop,
            // PTY demo commands (kept for backwards compat)
            commands::pty::pty_demo_start,
            commands::pty::pty_demo_write,
            commands::pty::pty_demo_resize,
            commands::pty::pty_demo_kill,
        ])
        .build(tauri::generate_context!())
        .expect("error building Sworm");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            let state = app_handle.state::<AppState>();
            // Drain batched PTY bytes into the writer queue, then kill
            // PTYs, then wait for the writer thread to finish persisting
            // everything before we let the process exit. Doing this in
            // order guarantees "a crash can lose at most the most recent
            // unflushed tail" holds on clean shutdown as well.
            state.transcript_batcher.flush_all();
            let cleaned = state.pty.kill_all();
            let lsp_cleaned = state.lsp.kill_all();
            state.transcript.shutdown_and_join();
            tracing::info!(
                "App exit cleanup finished, killed {} PTY sessions and {} LSP sessions",
                cleaned,
                lsp_cleaned
            );
        }
    });
}

/// Apply Linux-specific env overrides for GDK/WebKit.
///
/// Called before the Tauri runtime (and its threads) starts.
/// `std::env::set_var` is safe here because no other threads exist yet;
/// it will become `unsafe` in Rust 2024 edition; revisit when upgrading.
#[cfg(target_os = "linux")]
fn configure_linux_runtime_env() {
    if let Some(backend) = preferred_gdk_backend(std::env::var_os("GDK_BACKEND")) {
        std::env::set_var("GDK_BACKEND", backend);
    }

    if should_apply_webkit_workarounds() {
        set_env_if_missing("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        set_env_if_missing("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
    }
}

#[cfg(not(target_os = "linux"))]
fn configure_linux_runtime_env() {}

fn preferred_gdk_backend(current: Option<std::ffi::OsString>) -> Option<&'static str> {
    match current {
        Some(value) if !value.is_empty() => None,
        _ => Some("wayland,x11"),
    }
}

#[cfg(target_os = "linux")]
fn should_apply_webkit_workarounds() -> bool {
    should_apply_webkit_workarounds_for_env(
        std::env::var("SWORM_WEBKIT_WORKAROUNDS").ok().as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").is_some(),
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("GDK_BACKEND").ok().as_deref(),
    )
}

#[cfg(not(target_os = "linux"))]
fn should_apply_webkit_workarounds() -> bool {
    false
}

fn set_env_if_missing(key: &str, value: &str) {
    match std::env::var_os(key) {
        Some(existing) if !existing.is_empty() => {}
        _ => std::env::set_var(key, value),
    }
}

fn should_apply_webkit_workarounds_for_env(
    override_value: Option<&str>,
    has_wayland_display: bool,
    session_type: Option<&str>,
    gdk_backend: Option<&str>,
) -> bool {
    if let Some(value) = override_value {
        return value == "1";
    }

    if has_wayland_display {
        return true;
    }

    if session_type
        .map(|value| value.eq_ignore_ascii_case("wayland"))
        .unwrap_or(false)
    {
        return true;
    }

    gdk_backend
        .map(|value| value.split(',').any(|entry| entry.trim().eq("wayland")))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{preferred_gdk_backend, should_apply_webkit_workarounds_for_env};
    use std::ffi::OsString;

    #[test]
    fn defaults_to_wayland_first_when_backend_is_missing() {
        assert_eq!(preferred_gdk_backend(None), Some("wayland,x11"));
    }

    #[test]
    fn defaults_to_wayland_first_when_backend_is_empty() {
        assert_eq!(
            preferred_gdk_backend(Some(OsString::from(""))),
            Some("wayland,x11")
        );
    }

    #[test]
    fn preserves_existing_backend_override() {
        assert_eq!(preferred_gdk_backend(Some(OsString::from("x11"))), None);
    }

    // These test the pure `should_apply_webkit_workarounds_for_env` function
    // (no env access), so they run on any platform.
    #[test]
    fn webkit_workarounds_enable_for_wayland_session() {
        assert!(should_apply_webkit_workarounds_for_env(
            None,
            false,
            Some("wayland"),
            None
        ));
    }

    #[test]
    fn webkit_workarounds_respect_force_disable() {
        assert!(!should_apply_webkit_workarounds_for_env(
            Some("0"),
            true,
            Some("wayland"),
            Some("wayland,x11")
        ));
    }

    #[test]
    fn webkit_workarounds_respect_force_enable() {
        assert!(should_apply_webkit_workarounds_for_env(
            Some("1"),
            false,
            None,
            None
        ));
    }

    #[test]
    fn webkit_workarounds_enable_for_wayland_backend_hint() {
        assert!(should_apply_webkit_workarounds_for_env(
            None,
            false,
            Some("x11"),
            Some("wayland,x11")
        ));
    }
}
