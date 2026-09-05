mod app_state;
mod commands;
mod errors;
mod models;
mod services;

use crate::commands::app::launch_path_args;
use app_state::AppState;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    configure_linux_gdk_backend();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sworm_lib=info".into()),
        )
        .init();

    tracing::info!("Sworm starting up");

    let app = tauri::Builder::default()
        // Route warm launches through the same coordinator used by IPC and
        // cold-start restoration.
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            let state = app.state::<AppState>();
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let state = AppState::new(app.handle())?;
            let windows = Arc::clone(&state.windows);
            let manifest = {
                let db = state.db.write();
                windows
                    .load_manifest_or_migrate(app.handle(), &state.app_state_kv, db.conn())
                    .map_err(std::io::Error::other)?
            };
            app.manage(state);

            for entry in manifest.windows {
                windows
                    .create_workbench_window(app.handle(), Some(entry))
                    .map_err(std::io::Error::other)?;
            }

            let argv: Vec<String> = std::env::args().collect();
            let cwd = std::env::current_dir().ok();
            for path in launch_path_args(&argv, cwd.as_deref()) {
                windows.route_open_path(app.handle(), &path);
                tracing::info!("First-launch argv opened path: {path}");
            }

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
        .invoke_handler(tauri::generate_handler![
            // Activity map commands
            commands::activity_map::activity_map_get,
            commands::activity_map::activity_map_refresh,
            // App commands
            commands::app::clipboard_copy_files,
            commands::app::clipboard_read_files,
            commands::app::app_state_get,
            commands::app::app_state_put,
            commands::app::app_state_delete,
            commands::app::app_runtime_info,
            // Window commands
            commands::window::window_create,
            commands::window::window_ready,
            commands::window::window_get_label,
            commands::window::window_close,
            commands::window::window_claim_file,
            commands::window::window_release_file,
            commands::window::window_transfer_initiate,
            commands::window::window_transfer_source_exported,
            commands::window::window_transfer_target_staged,
            commands::window::window_transfer_abort,
            commands::window::pty_pause,
            commands::window::pty_attach,
            // Builtins commands
            commands::builtins::builtins_get_catalog,
            // Config schema commands (drives Monaco autocomplete for .sworm/*.json)
            commands::config_schemas::config_schemas_list,
            // Drag and drop commands
            commands::dnd::dnd_save_dropped_bytes,
            // Issue commands
            commands::issues::issues_list,
            commands::issues::issues_ready,
            commands::issues::issues_search,
            commands::issues::issues_get,
            commands::issues::issues_create,
            commands::issues::issues_update,
            commands::issues::issues_delete,
            commands::issues::issue_epics_create,
            commands::issues::issue_epics_list,
            commands::issues::issue_epics_get,
            commands::issues::issue_epics_update,
            commands::issues::issue_epics_delete,
            commands::issues::issue_comments_add,
            commands::issues::issue_comments_list,
            commands::issues::issue_comments_update,
            commands::issues::issue_comments_delete,
            commands::issues::issue_dependencies_add,
            commands::issues::issue_dependencies_remove,
            commands::issues::issue_dependencies_list,
            commands::issues::issue_current_git_user,
            commands::issues::issue_config_list,
            // Folder commands
            commands::folders::folder_select_directory,
            commands::folders::folder_resolve,
            commands::folders::folder_list_directories,
            commands::folders::folder_open_in_terminal,
            commands::folders::recent_folders_list,
            commands::folders::recent_folders_touch,
            commands::folders::recent_folders_remove,
            commands::folders::folder_claim,
            commands::folders::folder_release,
            // Provider commands
            commands::providers::provider_list,
            // Settings commands
            commands::settings::settings_get,
            commands::settings::settings_get_effective,
            commands::settings::settings_get_global_layer,
            commands::settings::settings_patch_global_section,
            commands::settings::settings_create_global_file,
            commands::settings::settings_open_global_file,
            commands::settings::settings_open_folder_file,
            commands::settings::settings_set_general,
            commands::settings::settings_set_formatting,
            commands::settings::settings_set_provider_config,
            // Shortcut commands
            commands::shortcuts::shortcuts_get_global,
            commands::shortcuts::shortcuts_set_global,
            commands::shortcuts::shortcuts_create_global_file,
            commands::shortcuts::shortcuts_open_global_file,
            // Formatter commands
            commands::formatting::formatting_format_biome,
            commands::formatting::formatting_format_nixfmt,
            // Task commands (folder-scoped .sworm/tasks.json)
            commands::tasks::tasks_list,
            commands::tasks::tasks_start,
            commands::tasks::tasks_write,
            commands::tasks::tasks_resize,
            commands::tasks::tasks_stop,
            // Nix environment commands
            commands::nix::nix_detect,
            commands::nix::nix_select,
            commands::nix::nix_evaluate,
            commands::nix::nix_clear,
            commands::nix::nix_lint,
            commands::nix::provider_list_for_folder,
            // File commands
            commands::files::file_read,
            commands::files::file_write,
            commands::files::file_create_dir,
            commands::files::file_rename,
            commands::files::file_delete,
            commands::files::file_paste,
            commands::files::file_paste_collisions,
            commands::files::files_read_dir,
            commands::files::files_list_paths,
            commands::files::files_watch_dirs,
            // Git commands
            commands::git::git_get_summary,
            commands::git::git_watch,
            commands::git::git_get_graph,
            commands::git::git_get_branch_commits,
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
            // Git branch commands
            commands::git::git_list_branches,
            commands::git::git_branch_status,
            commands::git::git_diff_branch_against_head,
            commands::git::git_checkout_branch,
            commands::git::git_checkout_remote_as_local,
            commands::git::git_create_branch,
            commands::git::git_rename_branch,
            commands::git::git_delete_branch,
            commands::git::git_delete_remote_branch,
            commands::git::git_set_upstream,
            commands::git::git_fast_forward_branch,
            commands::git::git_merge_into_current,
            commands::git::git_rebase_current_onto,
            commands::git::git_rebase_continue,
            commands::git::git_rebase_skip,
            commands::git::git_rebase_abort,
            commands::git::git_merge_abort,
            // LSP commands
            commands::lsp::lsp_list_servers,
            commands::lsp::lsp_set_server_config,
            commands::lsp::lsp_start,
            commands::lsp::lsp_send,
            commands::lsp::lsp_stop,
            // Session commands (process-only; resume identity lives in the tab)
            commands::sessions::session_start,
            commands::sessions::session_write,
            commands::sessions::session_resize,
            commands::sessions::session_stop,
            commands::sessions::omp_resolve_uri,
        ])
        .build(tauri::generate_context!())
        .expect("error building Sworm");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } => {
            let state = app_handle.state::<AppState>();
            state.windows.set_exit_requested(true);
            if let Err(error) = state.windows.save_manifest(app_handle) {
                tracing::error!("Failed to save window manifest on exit: {error}");
            }
        }
        tauri::RunEvent::Exit => {
            let state = app_handle.state::<AppState>();
            let cleaned = state.pty.kill_all();
            let lsp_cleaned = state.lsp.kill_all();
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
