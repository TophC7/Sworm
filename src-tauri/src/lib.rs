mod app_state;
mod commands;
mod errors;
mod models;
mod services;

use app_state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_linux_runtime_env();

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
            // Nix environment commands
            commands::nix::nix_detect,
            commands::nix::nix_select,
            commands::nix::nix_evaluate,
            commands::nix::nix_clear,
            commands::nix::nix_status,
            commands::nix::provider_list_for_project,
            // Git commands
            commands::git::git_get_summary,
            commands::git::git_get_file_diff,
            commands::git::git_get_diff_context,
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

/// Apply Linux-specific env overrides for GDK/WebKit.
///
/// Called before the Tauri runtime (and its threads) starts.
/// `std::env::set_var` is safe here because no other threads exist yet;
/// it will become `unsafe` in Rust 2024 edition — revisit when upgrading.
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
        std::env::var("ADE_WEBKIT_WORKAROUNDS").ok().as_deref(),
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
