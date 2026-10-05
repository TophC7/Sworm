pub mod app;
pub mod dnd;
pub mod files;
pub mod folders;
pub mod git;
pub mod lsp;
pub mod remote;
pub mod sessions;
pub mod settings;
pub mod shortcuts;
pub mod tasks;
pub mod window;
pub mod workbench;

use crate::app_state::AppState;
use sworm_core::errors::ApiError;

macro_rules! forward_command {
    (#[route($route:ident)] FolderClaim => $($row:tt)*) => {};
    (#[route($route:ident)] FolderRelease => $($row:tt)*) => {};
    (#[route($route:ident)] FilesWatchDirs => $($row:tt)*) => {};
    (#[route($route:ident)] GitWatch => $($row:tt)*) => {};
    (#[route($route:ident)] SessionStart => $($row:tt)*) => {};
    (#[route($route:ident)] TasksStart => $($row:tt)*) => {};
    (#[route($route:ident)] LspStart => $($row:tt)*) => {};
    (#[route($route:ident)] AppRuntimeInfo => $($row:tt)*) => {};
    (#[route($route:ident)] RunStatus => $($row:tt)*) => {};
    (#[route(connection)] $($row:tt)*) => {};
    (#[route(server)] $variant:ident => $method:ident(
        $($argument:ident: $argument_type:ty),* $(,)?
    ) -> $return_type:ty;) => {
        #[tauri::command]
        async fn $method(
            $($argument: $argument_type,)*
            folder_path: Option<String>,
            state: tauri::State<'_, AppState>,
        ) -> Result<$return_type, ApiError> {
            state
                .router
                .$method(settings::settings_server(folder_path.as_deref())?, $($argument),*)
                .await
        }
    };
    (#[route($route:ident)] $variant:ident => $method:ident(
        $($argument:ident: $argument_type:ty),* $(,)?
    ) -> $return_type:ty;) => {
        #[tauri::command]
        async fn $method(
            $($argument: $argument_type,)*
            state: tauri::State<'_, AppState>,
        ) -> Result<$return_type, ApiError> {
            state.router.$method($($argument),*).await
        }
    };
}

macro_rules! forward_commands {
    (
        $(
            #[route($route:ident)]
            $variant:ident => $method:ident(
                $($argument:ident: $argument_type:ty),* $(,)?
            ) -> $return_type:ty;
        )*
    ) => {
        $(
            forward_command! {
                #[route($route)]
                $variant => $method($($argument: $argument_type),*) -> $return_type;
            }
        )*
    };
}

sworm_protocol::sworm_rpc_ops!(forward_commands);

pub(crate) fn invoke_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        crate::deep_links::deep_link_take,
        remote::pair_remote,
        remote::repair_remote,
        remote::remote_client_fingerprint,
        remote::remote_status,
        workbench::workbench_list,
        workbench::workbench_close,
        workbench::workbench_attach,
        workbench::workbench_detach,
        workbench::workbench_transfer,
        workbench::workbench_save,
        remote::remote_runs_release,
        remote::rename_remote,
        remote::remove_remote,
        files::file_read_stream,
        files::file_read_stream_cancel,
        // Activity map commands
        activity_map_get,
        activity_map_refresh,
        // App commands
        app::clipboard_copy_files,
        app::clipboard_read_files,
        app_state_get,
        app_state_put,
        app_state_delete,
        app::app_runtime_info,
        // Window commands
        window::window_create,
        window::window_ready,
        window::window_close,
        window::window_claim_file,
        window::window_release_file,
        window::window_transfer_initiate,
        window::window_transfer_source_exported,
        window::window_transfer_target_staged,
        window::window_transfer_abort,
        window::window_group_handoff,
        window::window_group_exported,
        window::window_group_staged,
        window::window_group_abort,
        window::pty_pause,
        window::pty_attach,
        // Builtins commands
        builtins_get_catalog,
        // Config schema commands (drives Monaco autocomplete for .sworm/*.json)
        config_schemas_list,
        // Drag and drop commands
        dnd::dnd_save_dropped_bytes,
        // Issue commands
        issues_list,
        issues_ready,
        issues_search,
        issues_get,
        issues_create,
        issues_update,
        issues_delete,
        issue_epics_create,
        issue_epics_list,
        issue_epics_get,
        issue_epics_update,
        issue_epics_delete,
        issue_comments_add,
        issue_comments_list,
        issue_comments_update,
        issue_comments_delete,
        issue_dependencies_add,
        issue_dependencies_remove,
        issue_dependencies_list,
        issue_current_git_user,
        issue_config_list,
        // Folder commands
        folders::folder_select_directory,
        folder_resolve,
        folder_list_entries,
        folder_path_root,
        folder_home,
        folder_working_directory,
        folders::folder_open_in_terminal,
        recent_folders_list,
        recent_folders_touch,
        recent_folders_remove,
        folders::folder_claim,
        folders::folder_release,
        // Provider commands
        provider_list,
        // Settings commands
        settings_get,
        settings_get_effective,
        settings_get_global_layer,
        settings_patch_global_section,
        settings_create_global_file,
        settings::settings_open_global_file,
        settings_open_folder_file,
        settings_set_window,
        settings_set_terminal,
        settings_set_nix,
        settings_set_formatting,
        settings_set_provider_config,
        // Shortcut commands
        shortcuts_get_global,
        shortcuts_set_global,
        shortcuts_create_global_file,
        shortcuts::shortcuts_open_global_file,
        // Formatter commands
        formatting_format_biome,
        formatting_format_nixfmt,
        // Task commands (folder-scoped .sworm/tasks.jsonc)
        tasks_list,
        tasks::tasks_start,
        tasks::tasks_write,
        tasks::tasks_resize,
        tasks_stop,
        // Nix environment commands
        nix_detect,
        nix_select,
        nix_evaluate,
        nix_clear,
        nix_lint,
        provider_list_for_folder,
        // File commands
        file_read,
        file_stat,
        file_write,
        file_create_dir,
        file_rename,
        file_delete,
        file_paste,
        file_paste_collisions,
        files_read_dir,
        files_list_paths,
        files::files_watch_dirs,
        // Git commands
        git_get_summary,
        git_get_brief,
        git::git_watch,
        git_get_graph,
        git_get_branch_commits,
        git_get_commit_detail,
        diff_get_file,
        diff_get_working_index,
        // Git write commands
        git_stage_all,
        git_stage_files,
        git_unstage_all,
        git_unstage_files,
        git_discard_all,
        git_discard_files,
        git_get_full_patch,
        git_get_path_patch,
        git_get_quick_diff_data,
        git_stage_file_content,
        git_commit,
        git_undo_last_commit,
        git_push,
        git_push_force_with_lease,
        git_pull,
        git_fetch,
        git_stash_all,
        git_stash_count,
        git_stash_list,
        git_stash_pop,
        git_stash_drop,
        git_show_file,
        git_init,
        git_clone_in_place,
        // Git branch commands
        git_list_branches,
        git_branch_status,
        git_diff_branch_against_head,
        git_checkout_branch,
        git_checkout_remote_as_local,
        git_create_branch,
        git_rename_branch,
        git_delete_branch,
        git_delete_remote_branch,
        git_set_upstream,
        git_fast_forward_branch,
        git_merge_into_current,
        git_rebase_current_onto,
        git_rebase_continue,
        git_rebase_skip,
        git_rebase_abort,
        git_merge_abort,
        // LSP commands
        lsp_list_servers,
        lsp_set_server_config,
        lsp::lsp_start,
        lsp::lsp_send,
        lsp_stop,
        // Session commands (process-only; resume identity lives in the tab)
        sessions::session_start,
        sessions::session_write,
        sessions::session_resize,
        session_stop,
        omp_resolve_uri,
    ]
}
