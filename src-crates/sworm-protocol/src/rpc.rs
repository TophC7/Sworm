use serde::{Deserialize, Serialize};

/// Protocol version selected during the QUIC TLS handshake. Bump on any
/// wire-incompatible change once released; there is no negotiation.
pub const ALPN: &[u8] = b"sworm/1";
/// Maximum encoded `Open` frame body accepted before a connection is paired,
/// and the ceiling every non-RPC open is written with: pairing metadata,
/// stream ids, and cursors are tiny, so an unauthenticated peer can never
/// make the daemon allocate more than this.
pub const MAX_REQUEST_FRAME_BYTES: usize = 64 * 1024;
/// Maximum JSON or raw frame body.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
/// Whole-file read/write ceiling, local and remote alike, well below the
/// response frame ceiling; larger files stream in chunks after approval.
pub const MAX_WHOLE_FILE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STREAM_FILE_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_FILE_CHUNK_BYTES: usize = 1024 * 1024;
/// One events stream, PTY streams, and concurrent RPC streams.
pub const MAX_STREAMS_PER_CONNECTION: u32 = 256;

/// Cursor for independently replaying terminal bytes and lifecycle events.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PtyCursor {
    /// Total output bytes already consumed.
    pub output_offset: u64,
    /// Highest lifecycle event sequence already consumed.
    pub event_sequence: u64,
}

/// Whether a run can still be attached, and its exit state when known.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunStatus {
    pub live: bool,
    /// `None` means no completed run is retained; `Some(None)` means it exited
    /// without a code; `Some(Some(code))` carries its exit code.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_exit"
    )]
    pub exited: Option<Option<i32>>,
}

/// A durable workbench as Home lists it. `connected` and `running` are live
/// observations, never persisted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchInfo {
    pub id: String,
    pub created_at: String,
    pub last_seen_at: String,
    pub connected: bool,
    /// Label of the client that last attached (`"browser"` or a desktop's
    /// host name); with `connected`, the one controlling it now.
    pub client: Option<String>,
    /// The caller itself holds the live lease.
    pub yours: bool,
    /// Distinct tab folder paths from the saved V4 snapshot: the active tab's
    /// folder first, then tab order. Empty when malformed/unrecognized.
    pub folders: Vec<String>,
    /// Live, noncompleted runs owned by this workbench.
    pub running: Vec<WorkbenchRun>,
}

/// How a client asks for a workbench's controller lease. Braced variants so
/// `deny_unknown_fields` rejects a token sent with `open`/`takeover`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttachMode {
    /// Take control if nobody holds it; otherwise Busy.
    Open {},
    /// Reclaim this client's own lease after a reconnect; Revoked if superseded.
    Resume { controller_token: String },
    /// Revoke the current controller. A taker has no authority to prove.
    Takeover {},
}

/// Outcome of a desktop's workbench attach. Every outcome carries the saved
/// V4 snapshot, so a window can show the tabs it does not control.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchAttached {
    Ready {
        /// Caller-generated identity of this connection's attach operation.
        attachment_id: String,
        controller_token: String,
        snapshot: String,
    },
    /// Another client holds the lease; Takeover revokes it.
    Busy {
        client: Option<String>,
        snapshot: String,
    },
    /// The supplied lease token was superseded while this client was away.
    Revoked {
        client: Option<String>,
        snapshot: String,
    },
}

/// A live run owned by a durable web workbench.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbenchRun {
    /// `provider_id` is `"terminal"` for shells.
    Session {
        folder: String,
        provider_id: String,
    },
    Task {
        folder: String,
        task_id: String,
    },
}

/// A recently opened folder; `opened_at` is RFC 3339 UTC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentFolder {
    pub path: String,
    pub opened_at: String,
}

fn deserialize_exit<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<i32>>, D::Error> {
    Option::<i32>::deserialize(deserializer).map(Some)
}

/// Invoke a callback macro with every RPC operation.
///
/// Callback rows have this grammar:
/// `#[route(key)] Variant => method(arg: Type, ...) -> ReturnType;`.
///
/// Route keys name how the desktop router picks a host for the call:
/// - `project_path`, `path`, `folder_path`: that argument is a workspace path.
/// - `opt_folder_path`: an optional `folder_path`; `None` is local.
/// - `server`: no path at all; the caller passes the server explicitly.
/// - `run`: keyed by a registered run id.
/// - `lsp`: keyed by a registered LSP session id.
/// - `connection`: handled by the connection/workbench-registry layer; never reaches Host.
/// - `none`: host-global; always the local host.
#[macro_export]
macro_rules! sworm_rpc_ops {
    ($callback:ident) => {
        $callback! {
            #[route(none)]
            AppStateGet => app_state_get(key: String) -> Option<String>;
            #[route(none)]
            AppStatePut => app_state_put(key: String, value_json: String) -> ();
            #[route(none)]
            AppStateDelete => app_state_delete(key: String) -> ();
            #[route(connection)]
            WorkbenchList => workbench_list() -> Vec<$crate::rpc::WorkbenchInfo>;
            #[route(connection)]
            WorkbenchClose => workbench_close(id: String) -> ();
            // Desktop lease: binds `id` to this QUIC connection until
            // matching `workbench_detach`, a takeover, Close, or disconnect.
            // Use a fresh attachment_id per operation; recover a lost response
            // on this same connection instead of replaying the attach.
            // Web pages attach through their socket hello instead.
            #[route(connection)]
            WorkbenchAttach => workbench_attach(
                id: String,
                attachment_id: String,
                mode: $crate::rpc::AttachMode,
                client: String,
            ) -> $crate::rpc::WorkbenchAttached;
            #[route(connection)]
            WorkbenchDetach => workbench_detach(id: String, attachment_id: String) -> ();
            // Wait for the matching pending attempt. Return Ready with the
            // current snapshot only while its control remains admitted;
            // unknown, superseded or retired attempts return None. Never attach.
            // Unknown recovery fences that exact late attach on this connection.
            // Bounded cancellation state never evicts identities: reconnect if
            // its capacity error rejects new attaches or unknown recovery.
            #[route(connection)]
            WorkbenchRecover => workbench_recover(
                id: String,
                attachment_id: String,
            ) -> Option<$crate::rpc::WorkbenchAttached>;
            // Store the snapshot of a workbench this connection controls.
            #[route(connection)]
            WorkbenchSave => workbench_save(id: String, snapshot: String) -> ();
            #[route(none)]
            AppRuntimeInfo => app_runtime_info() -> $crate::app::AppRuntimeInfo;
            #[route(none)]
            RecentFoldersList => recent_folders_list() -> Vec<$crate::rpc::RecentFolder>;
            #[route(none)]
            RecentFoldersTouch => recent_folders_touch(path: String) -> Vec<$crate::rpc::RecentFolder>;
            #[route(none)]
            RecentFoldersRemove => recent_folders_remove(paths: Vec<String>) -> Vec<$crate::rpc::RecentFolder>;
            #[route(folder_path)]
            FolderClaim => folder_claim(folder_path: String) -> ();
            #[route(folder_path)]
            FolderRelease => folder_release(folder_path: String) -> ();
            #[route(path)]
            FolderPathRoot => folder_path_root(path: String) -> $crate::folder::PathRoot;
            #[route(server)]
            FolderHome => folder_home() -> String;
            #[route(server)]
            FolderWorkingDirectory => folder_working_directory() -> String;
            #[route(none)]
            ProviderList => provider_list() -> Vec<$crate::provider::ProviderStatus>;
            #[route(none)]
            ActivityMapGet => activity_map_get() -> Vec<$crate::activity_map::DiscoveredProject>;
            #[route(none)]
            ActivityMapRefresh => activity_map_refresh() -> Vec<$crate::activity_map::DiscoveredProject>;
            #[route(none)]
            BuiltinsGetCatalog => builtins_get_catalog() -> $crate::builtins::BuiltinCatalog;
            #[route(none)]
            ConfigSchemasList => config_schemas_list() -> Vec<$crate::config_schemas::ConfigSchemaEntry>;
            #[route(none)]
            OmpResolveUri => omp_resolve_uri(uri: String, cwd: Option<String>) -> $crate::omp::OmpResolvedTarget;
            #[route(none)]
            SettingsGetGlobalLayer => settings_get_global_layer() -> $crate::settings::SettingsLayerPayload;
            #[route(none)]
            SettingsCreateGlobalFile => settings_create_global_file() -> $crate::settings::SettingsFileResult;
            #[route(none)]
            SettingsSetWindow => settings_set_window(settings: $crate::settings::WindowSettings) -> $crate::settings::WindowSettings;
            #[route(none)]
            SettingsSetTypography => settings_set_typography(settings: $crate::settings::TypographySettings) -> $crate::settings::TypographySettings;
            #[route(none)]
            ShortcutsGetGlobal => shortcuts_get_global() -> $crate::settings::ShortcutsFilePayload;
            #[route(none)]
            ShortcutsSetGlobal => shortcuts_set_global(value: serde_json::Value) -> $crate::settings::ShortcutsFilePayload;
            #[route(none)]
            ShortcutsCreateGlobalFile => shortcuts_create_global_file() -> $crate::settings::SettingsFileResult;
            #[route(project_path)]
            FilesReadDir => files_read_dir(
                project_path: String,
                dir_path: String,
                show_hidden: bool,
            ) -> Vec<$crate::files::DirEntry>;
            #[route(project_path)]
            FileRead => file_read(
                project_path: String,
                file_path: String,
            ) -> $crate::files::FileContent;
            #[route(project_path)]
            FileStat => file_stat(
                project_path: String,
                file_path: String,
            ) -> $crate::files::FileStat;
            #[route(project_path)]
            FileWrite => file_write(
                project_path: String,
                file_path: String,
                content: String,
                expected_version: Option<String>,
            ) -> String;
            #[route(project_path)]
            FileCreateDir => file_create_dir(
                project_path: String,
                dir_path: String,
            ) -> ();
            #[route(project_path)]
            FileRename => file_rename(
                project_path: String,
                old_path: String,
                new_path: String,
            ) -> ();
            #[route(project_path)]
            FilePaste => file_paste(
                project_path: String,
                target_dir: String,
                op: String,
                sources: Vec<String>,
                collision_policy: String,
                rename_map: Option<std::collections::HashMap<String, String>>,
                permanent: bool,
            ) -> Vec<$crate::files::FilePasteMapping>;
            #[route(project_path)]
            FilePasteCollisions => file_paste_collisions(
                project_path: String,
                target_dir: String,
                sources: Vec<String>,
            ) -> Vec<$crate::files::FilePasteCollision>;
            #[route(project_path)]
            FileDelete => file_delete(
                project_path: String,
                file_path: String,
                permanent: bool,
            ) -> ();
            #[route(project_path)]
            FilesListPaths => files_list_paths(
                project_path: String,
                show_hidden: bool,
            ) -> $crate::files::PathList;
            #[route(path)]
            GitGetSummary => git_get_summary(
                path: String,
            ) -> $crate::git::GitSummary;
            #[route(path)]
            GitGetBrief => git_get_brief(
                path: String,
            ) -> $crate::git::GitBrief;
            #[route(path)]
            FolderResolve => folder_resolve(
                path: String,
            ) -> $crate::folder::FolderInfo;
            #[route(path)]
            FolderListEntries => folder_list_entries(
                path: String,
                show_hidden: bool,
            ) -> Vec<$crate::folder::FolderEntry>;
            #[route(project_path)]
            FilesWatchDirs => files_watch_dirs(
                project_path: String,
                dirs: Vec<String>,
            ) -> ();
            #[route(project_path)]
            GitWatch => git_watch(
                project_path: String,
            ) -> ();
            #[route(path)]
            GitGetCommitDetail => git_get_commit_detail(
                path: String,
                hash: String,
            ) -> Option<$crate::git::CommitDetail>;
            #[route(path)]
            DiffGetFile => diff_get_file(
                path: String,
                source: $crate::file_diff::DiffSource,
                file_path: String,
                old_path: Option<String>,
                status: $crate::file_diff::GitStatus,
            ) -> $crate::git::DiffFileContent;
            #[route(path)]
            DiffGetWorkingIndex => diff_get_working_index(
                path: String,
                staged: bool,
            ) -> Vec<$crate::file_diff::FileDiff>;
            #[route(path)]
            GitGetGraph => git_get_graph(
                path: String,
                limit: usize,
            ) -> Vec<$crate::git::GraphCommit>;
            #[route(path)]
            GitGetBranchCommits => git_get_branch_commits(
                path: String,
                branch: String,
                limit: usize,
            ) -> Vec<$crate::git::GraphCommit>;
            #[route(path)]
            GitPaths => git_paths(
                path: String,
                op: $crate::git::GitPathsOp,
            ) -> ();
            #[route(path)]
            GitGetFullPatch => git_get_full_patch(
                path: String,
            ) -> Option<String>;
            #[route(path)]
            GitGetPathPatch => git_get_path_patch(
                path: String,
                files: Vec<String>,
                staged: Option<bool>,
            ) -> Option<String>;
            #[route(project_path)]
            GitGetQuickDiffData => git_get_quick_diff_data(
                project_path: String,
                file_path: String,
            ) -> $crate::git::GitQuickDiffData;
            #[route(project_path)]
            GitStageFileContent => git_stage_file_content(
                project_path: String,
                file_path: String,
                content: Option<String>,
            ) -> ();
            #[route(path)]
            GitCommit => git_commit(
                path: String,
                message: String,
                options: $crate::git::GitCommitOptions,
            ) -> String;
            #[route(path)]
            GitUndoLastCommit => git_undo_last_commit(
                path: String,
            ) -> String;
            #[route(path)]
            GitRemote => git_remote(
                path: String,
                op: $crate::git::GitRemoteOp,
            ) -> ();
            #[route(path)]
            GitStash => git_stash(
                path: String,
                op: $crate::git::GitStashOp,
            ) -> ();
            #[route(path)]
            GitStashCount => git_stash_count(
                path: String,
            ) -> usize;
            #[route(path)]
            GitStashList => git_stash_list(
                path: String,
            ) -> Vec<$crate::git::StashEntry>;
            #[route(project_path)]
            GitShowFile => git_show_file(
                project_path: String,
                git_ref: String,
                file_path: String,
            ) -> String;
            #[route(path)]
            GitInit => git_init(
                path: String,
            ) -> ();
            #[route(path)]
            GitCloneInPlace => git_clone_in_place(
                path: String,
                url: String,
            ) -> ();
            #[route(path)]
            GitListBranches => git_list_branches(
                path: String,
            ) -> Vec<$crate::branch::BranchSummary>;
            #[route(path)]
            GitListRemotes => git_list_remotes(
                path: String,
            ) -> Vec<$crate::git::GitRemote>;
            #[route(path)]
            GitWorktreeList => git_worktree_list(
                path: String,
            ) -> $crate::git::GitWorktreeList;
            #[route(path)]
            GitWorktreeDefaultPath => git_worktree_default_path(
                path: String,
                checkout: $crate::git::GitWorktreeCheckout,
                root: Option<String>,
            ) -> String;
            #[route(path)]
            GitWorktree => git_worktree(
                path: String,
                op: $crate::git::GitWorktreeOp,
            ) -> $crate::git::GitWorktreeResult;
            #[route(path)]
            GitBranch => git_branch(
                path: String,
                op: $crate::git::GitBranchOp,
            ) -> ();
            #[route(path)]
            GitListTags => git_list_tags(path: String) -> Vec<$crate::git::GitTag>;
            #[route(path)]
            GitTag => git_tag(path: String, op: $crate::git::GitTagOp) -> ();
            #[route(path)]
            GitRemoteConfig => git_remote_config(path: String, op: $crate::git::GitRemoteConfigOp) -> ();
            #[route(path)]
            GitHistory => git_history(path: String, op: $crate::git::GitHistoryOp) -> ();
            #[route(path)]
            GitDiffRange => git_diff_range(
                path: String,
                base: String,
                target: Option<String>,
            ) -> Vec<$crate::file_diff::FileDiff>;
            #[route(path)]
            GitMergedBranches => git_merged_branches(path: String) -> Vec<String>;
            #[route(path)]
            GitSequencer => git_sequencer(
                path: String,
                operation: $crate::git::GitOperationKind,
                action: $crate::git::GitSequencerAction,
            ) -> ();
            #[route(path)]
            GitConflicts => git_conflicts(
                path: String,
                op: $crate::git::GitConflictsOp,
            ) -> ();
            #[route(folder_path)]
            IssuesList => issues_list(
                folder_path: String,
                filters: $crate::issues::IssueListFilters,
            ) -> Vec<$crate::issues::Issue>;
            #[route(folder_path)]
            IssuesReady => issues_ready(
                folder_path: String,
                filters: $crate::issues::IssueReadyFilters,
            ) -> Vec<$crate::issues::Issue>;
            #[route(folder_path)]
            IssuesSearch => issues_search(
                folder_path: String,
                query: String,
                filters: $crate::issues::IssueListFilters,
            ) -> Vec<$crate::issues::Issue>;
            #[route(folder_path)]
            IssuesGet => issues_get(
                folder_path: String,
                issue_id: String,
            ) -> $crate::issues::IssueDetail;
            #[route(folder_path)]
            IssuesCreate => issues_create(
                folder_path: String,
                input: $crate::issues::IssueCreateInput,
            ) -> $crate::issues::Issue;
            #[route(folder_path)]
            IssuesUpdate => issues_update(
                folder_path: String,
                issue_id: String,
                patch: $crate::issues::IssueUpdateInput,
            ) -> $crate::issues::Issue;
            #[route(folder_path)]
            IssuesDelete => issues_delete(
                folder_path: String,
                issue_id: String,
            ) -> ();
            #[route(folder_path)]
            IssueEpicsCreate => issue_epics_create(
                folder_path: String,
                input: $crate::issues::IssueEpicCreateInput,
            ) -> $crate::issues::IssueEpic;
            #[route(folder_path)]
            IssueEpicsList => issue_epics_list(
                folder_path: String,
            ) -> Vec<$crate::issues::IssueEpic>;
            #[route(folder_path)]
            IssueEpicsGet => issue_epics_get(
                folder_path: String,
                epic_id: String,
            ) -> $crate::issues::IssueEpic;
            #[route(folder_path)]
            IssueEpicsUpdate => issue_epics_update(
                folder_path: String,
                epic_id: String,
                patch: $crate::issues::IssueEpicUpdateInput,
            ) -> $crate::issues::IssueEpic;
            #[route(folder_path)]
            IssueEpicsDelete => issue_epics_delete(
                folder_path: String,
                epic_id: String,
            ) -> ();
            #[route(folder_path)]
            IssueCommentsAdd => issue_comments_add(
                folder_path: String,
                input: $crate::issues::IssueCommentCreateInput,
            ) -> $crate::issues::IssueComment;
            #[route(folder_path)]
            IssueCommentsList => issue_comments_list(
                folder_path: String,
                issue_id: String,
            ) -> Vec<$crate::issues::IssueComment>;
            #[route(folder_path)]
            IssueCommentsUpdate => issue_comments_update(
                folder_path: String,
                comment_id: String,
                input: $crate::issues::IssueCommentUpdateInput,
            ) -> $crate::issues::IssueComment;
            #[route(folder_path)]
            IssueCommentsDelete => issue_comments_delete(
                folder_path: String,
                comment_id: String,
            ) -> ();
            #[route(folder_path)]
            IssueDependenciesAdd => issue_dependencies_add(
                folder_path: String,
                input: $crate::issues::IssueDependencyInput,
            ) -> $crate::issues::IssueDependency;
            #[route(folder_path)]
            IssueDependenciesRemove => issue_dependencies_remove(
                folder_path: String,
                input: $crate::issues::IssueDependencyInput,
            ) -> ();
            #[route(folder_path)]
            IssueDependenciesList => issue_dependencies_list(
                folder_path: String,
                issue_id: String,
            ) -> Vec<$crate::issues::IssueDependency>;
            #[route(folder_path)]
            IssueCurrentGitUser => issue_current_git_user(
                folder_path: String,
            ) -> String;
            #[route(folder_path)]
            IssueConfigList => issue_config_list(
                folder_path: String,
            ) -> Vec<$crate::issues::IssueConfigEntry>;
            #[route(folder_path)]
            NixDetect => nix_detect(
                folder_path: String,
            ) -> $crate::nix_env::NixDetection;
            #[route(folder_path)]
            NixSelect => nix_select(
                folder_path: String,
                nix_file: String,
            ) -> $crate::nix_env::NixEnvRecord;
            #[route(folder_path)]
            NixEvaluate => nix_evaluate(
                folder_path: String,
            ) -> $crate::nix_env::NixEnvRecord;
            #[route(folder_path)]
            NixClear => nix_clear(
                folder_path: String,
            ) -> ();
            #[route(folder_path)]
            NixLint => nix_lint(
                folder_path: String,
                file_path: String,
            ) -> Vec<$crate::nix_env::NixDiagnostic>;
            #[route(folder_path)]
            ProviderListForFolder => provider_list_for_folder(
                folder_path: String,
            ) -> Vec<$crate::provider::ProviderStatus>;
            #[route(folder_path)]
            FormattingFormatBiome => formatting_format_biome(
                folder_path: String,
                file_path: String,
                content: String,
            ) -> String;
            #[route(folder_path)]
            FormattingFormatNixfmt => formatting_format_nixfmt(
                folder_path: String,
                content: String,
            ) -> String;
            #[route(opt_folder_path)]
            SettingsGetEffective => settings_get_effective(
                folder_path: Option<String>,
            ) -> $crate::settings::EffectiveSettingsPayload;
            #[route(server)]
            SettingsGet => settings_get() -> $crate::settings::SettingsPayload;
            #[route(server)]
            SettingsPatchGlobalSection => settings_patch_global_section(
                input: $crate::settings::PatchSettingsSectionInput,
            ) -> $crate::settings::SettingsLayerPayload;
            #[route(server)]
            SettingsSetNix => settings_set_nix(
                settings: $crate::settings::NixSettings,
            ) -> $crate::settings::NixSettings;
            #[route(server)]
            SettingsSetFormatting => settings_set_formatting(
                formatting: $crate::settings::FormattingSettings,
            ) -> $crate::settings::FormattingSettings;
            #[route(server)]
            SettingsSetProviderConfig => settings_set_provider_config(
                config: $crate::settings::ProviderConfigRecord,
            ) -> $crate::settings::ProviderConfigRecord;
            #[route(folder_path)]
            SettingsOpenFolderFile => settings_open_folder_file(
                folder_path: String,
            ) -> $crate::settings::SettingsFileResult;
            #[route(opt_folder_path)]
            LspListServers => lsp_list_servers(
                folder_path: Option<String>,
            ) -> Vec<$crate::lsp::LspServerSettingsEntry>;
            #[route(server)]
            LspSetServerConfig => lsp_set_server_config(
                config: $crate::settings::LspServerConfigRecord,
            ) -> $crate::settings::LspServerConfigRecord;
            #[route(folder_path)]
            LspStart => lsp_start(
                session_id: String,
                folder_path: String,
                server_definition_id: String,
                root_path: String,
            ) -> ();
            #[route(lsp)]
            LspStop => lsp_stop(
                session_id: String,
            ) -> ();
            #[route(folder_path)]
            TasksList => tasks_list(
                folder_path: String,
            ) -> Vec<$crate::task::TaskDefinition>;
            #[route(folder_path)]
            SessionStart => session_start(
                run_id: String,
                folder_path: String,
                provider_id: String,
                resume_token: Option<String>,
                cols: u16,
                rows: u16,
            ) -> $crate::session::SessionStartInfo;
            #[route(folder_path)]
            TasksStart => tasks_start(
                run_id: String,
                folder_path: String,
                task_id: String,
                active_file_path: Option<String>,
                cols: u16,
                rows: u16,
                attach_only: bool,
            ) -> ();
            #[route(run)]
            SessionStop => session_stop(
                run_id: String,
            ) -> ();
            #[route(run)]
            TasksStop => tasks_stop(
                run_id: String,
            ) -> ();
            #[route(run)]
            RunStatus => run_status(
                run_id: String,
            ) -> $crate::rpc::RunStatus;
            #[route(connection)]
            Pair => pair(
                token: String,
                name: String,
            ) -> ();
        }
    };
}

macro_rules! define_rpc {
    (
        $(
            #[route($route:ident)]
            $variant:ident => $method:ident(
                $($argument:ident: $argument_type:ty),* $(,)?
            ) -> $return_type:ty;
        )*
    ) => {
        /// Operation request. Path fields name absolute paths on the daemon.
        ///
        /// Argument types come from the whole workspace surface, so this
        /// derives no equality: comparing requests is not something the
        /// transport needs, and every payload type would have to opt in.
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(tag = "method", content = "params", rename_all = "snake_case")]
        pub enum Request {
            $(
                $variant {
                    $($argument: $argument_type),*
                },
            )*
        }

        /// Typed operation result.
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[serde(tag = "method", content = "params", rename_all = "snake_case")]
        pub enum Reply {
            $(
                $variant($return_type),
            )*
        }

        impl Reply {
            $(
                /// Consume this reply and extract this operation's typed value.
                pub fn $method(self) -> Result<$return_type, WireError> {
                    match self {
                        Self::$variant(value) => Ok(value),
                        _ => Err(WireError::Internal {
                            message: concat!(
                                "unexpected reply variant for ",
                                stringify!($method),
                            )
                            .to_owned(),
                        }),
                    }
                }
            )*
        }
    };
}

sworm_rpc_ops!(define_rpc);

pub type Response = Result<Reply, WireError>;

/// First JSON frame on every bidirectional stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Open {
    Rpc(Request),
    /// An RPC under a workbench lease this connection holds: runs it starts
    /// belong to that workbench.
    WorkbenchRpc {
        workbench: String,
        request: Request,
    },
    Events,
    Pty {
        run_id: String,
        cursor: PtyCursor,
    },
    Lsp {
        session_id: String,
    },
    FileRead {
        project_path: String,
        file_path: String,
        version: String,
    },
}

/// Raw file chunks precede exactly one terminal JSON frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileReadDown {
    Complete { version: String },
    Error { error: WireError },
}

/// `Open::Rpc` without owning the request.
///
/// A request can carry a whole file body, so the client writes this instead
/// of cloning one to build the owned variant. It serializes to exactly the
/// bytes `Open::Rpc` does, which `open_rpc_flattens_the_typed_request` pins.
#[derive(Debug, Serialize)]
pub struct OpenRpc<'a> {
    kind: &'static str,
    #[serde(flatten)]
    request: &'a Request,
}

impl<'a> OpenRpc<'a> {
    pub fn new(request: &'a Request) -> Self {
        Self {
            kind: "rpc",
            request,
        }
    }
}

/// `Open::WorkbenchRpc` without owning the request; same bytes as the owned frame.
#[derive(Debug, Serialize)]
pub struct OpenWorkbenchRpc<'a> {
    kind: &'static str,
    workbench: &'a str,
    request: &'a Request,
}

impl<'a> OpenWorkbenchRpc<'a> {
    pub fn new(workbench: &'a str, request: &'a Request) -> Self {
        Self {
            kind: "workbench_rpc",
            workbench,
            request,
        }
    }
}

/// Desktop-to-daemon JSON frames on an LSP stream.
///
/// Messages are ordered like PTY input: a language server rejects requests
/// that arrive out of order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspUp {
    Message { payload_json: String },
}

/// Daemon-to-desktop JSON frames on an LSP stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LspDown {
    /// The daemon registered this session's sink; `lsp_start` may proceed.
    Ready,
    Event {
        event: crate::lsp::LspEvent,
    },
}

/// Daemon-to-desktop JSON control frames on a PTY stream.
///
/// Terminal output uses a raw frame whose first eight body bytes are the
/// big-endian output offset of the first byte that follows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PtyDown {
    Event {
        sequence: u64,
        event: crate::pty::PtyEvent,
    },
    Gap {
        lost_bytes: u64,
    },
    /// Terminal frame: the daemon cannot serve this run, so the desktop must
    /// stop reattaching instead of retrying a request that can never succeed.
    Closed {
        error: WireError,
    },
}

/// Desktop-to-daemon JSON control frames on a PTY stream.
///
/// Terminal input uses raw frames.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PtyUp {
    Resize { cols: u16, rows: u16 },
}

/// Wire-safe mirror of `sworm_core::events::HostEvent`.
///
/// Mutation bookkeeping (`FileMoved` and `FileDeleted`) is intentionally
/// absent: those describe a mutation the desktop itself requested, so the
/// router emits them locally with URI paths after the remote call succeeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum HostEventWire {
    FilesChanged(crate::files::FilesChangedEvent),
    GitChanged(crate::git::GitChangedEvent),
    SettingsChanged(crate::settings::SettingsChangedEvent),
    TasksChanged(String),
    NixChanged(String),
    IssuesChanged(String),
    RecentFoldersChanged(Vec<RecentFolder>),
    /// The workbench registry changed: an attach, takeover, detach, or Close.
    WorkbenchesChanged(()),
}

/// Wire mirror of `sworm_core::errors::ApiError` plus transport-level auth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireError {
    Database {
        message: String,
    },
    Pty {
        message: String,
    },
    Io {
        message: String,
    },
    NotFound {
        message: String,
    },
    InvalidArgument {
        message: String,
    },
    Internal {
        message: String,
    },
    BranchUnmerged {
        branch: String,
        message: String,
    },
    DirtyWorktree {
        message: String,
    },
    /// The OS trash refused the item; the caller may retry as a permanent delete.
    TrashUnavailable {
        message: String,
    },
    /// Push/pull/sync needs an upstream the branch does not have; the UI offers Publish.
    NoUpstream {
        branch: String,
    },
    /// The remote refused the push (non-fast-forward or stale lease).
    PushRejected {
        message: String,
    },
    /// The command stopped on merge conflicts, or refused because unmerged paths remain.
    MergeConflicts {
        message: String,
    },
    /// The branch is checked out in another worktree.
    BranchInWorktree {
        branch: String,
        path: String,
    },
    /// A linked worktree refused removal because it has modified or untracked files.
    WorktreeHasChanges {
        path: String,
        message: String,
    },
    /// Staging would record files that still contain conflict markers.
    ConflictMarkers {
        paths: Vec<String>,
    },
    Unauthorized {
        message: String,
    },
    /// A write lost a race with another writer; `current_version` is what is
    /// on disk now, so the caller can reload or overwrite deliberately.
    Conflict {
        current_version: String,
    },
    /// A conditional write found nothing at `path`: it was deleted after the
    /// caller read it, so recreating it has to be asked for explicitly.
    Deleted {
        path: String,
    },
    /// A start was refused because the session id already has a live server on
    /// the host. Typed so only the id's owner replaces its own orphan.
    LspAlreadyActive {
        session_id: String,
    },
    /// A file exceeds a read ceiling. Typed so the editor can offer the
    /// streamed path instead of matching on message text.
    TooLarge {
        size: u64,
        limit: u64,
    },
    /// A workbench-scoped request found no live lease for `workbench` on this
    /// connection: it was taken over, closed, or never attached.
    NotController {
        workbench: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_identity_is_required_at_control_boundaries() {
        for method in ["workbench_attach", "workbench_detach", "workbench_recover"] {
            let mut params = serde_json::json!({ "id": "workbench" });
            if method == "workbench_attach" {
                params["mode"] = serde_json::json!({ "kind": "open" });
                params["client"] = serde_json::json!("desktop");
            }
            let mut request = serde_json::json!({ "method": method, "params": params });
            assert!(serde_json::from_value::<Request>(request.clone()).is_err());
            request["params"]["attachment_id"] = serde_json::json!("operation");
            assert!(serde_json::from_value::<Request>(request).is_ok());
        }
        let mut ready = serde_json::json!({
            "kind": "ready", "controller_token": "token", "snapshot": "{}"
        });
        assert!(serde_json::from_value::<WorkbenchAttached>(ready.clone()).is_err());
        ready["attachment_id"] = serde_json::json!("operation");
        assert!(matches!(
            serde_json::from_value::<WorkbenchAttached>(ready).unwrap(),
            WorkbenchAttached::Ready { attachment_id, .. } if attachment_id == "operation"
        ));
    }

    #[test]
    fn run_status_preserves_unknown_and_completed_without_exit_code() {
        for exited in [None, Some(None), Some(Some(7))] {
            let status = RunStatus {
                live: false,
                exited,
            };
            let encoded = serde_json::to_vec(&status).unwrap();
            assert_eq!(
                serde_json::from_slice::<RunStatus>(&encoded).unwrap(),
                status
            );
        }
    }

    #[test]
    fn operation_variants_match_wire_method_names() {
        macro_rules! collect_methods {
            (
                $(
                    #[route($route:ident)]
                    $variant:ident => $method:ident(
                        $($argument:ident: $argument_type:ty),* $(,)?
                    ) -> $return_type:ty;
                )*
            ) => {
                vec![$((stringify!($variant), stringify!($method))),*]
            };
        }

        let methods: Vec<(&str, &str)> = sworm_rpc_ops!(collect_methods);
        for (variant, method) in methods {
            // Match serde's snake_case enum-variant tags, including acronym boundaries.
            let mut wire_method = String::new();
            for (index, ch) in variant.char_indices() {
                if index > 0 && ch.is_uppercase() {
                    wire_method.push('_');
                }
                wire_method.extend(ch.to_lowercase());
            }
            assert_eq!(wire_method, method, "wire method for {variant}");
        }
    }

    #[test]
    fn recent_folders_event_serializes_as_host_global_payload() {
        let folders = vec![
            RecentFolder {
                path: "sworm://host/repo".to_owned(),
                opened_at: "2026-01-02T03:04:05+00:00".to_owned(),
            },
            RecentFolder {
                path: "/local/repo".to_owned(),
                opened_at: "2026-01-01T03:04:05+00:00".to_owned(),
            },
        ];
        let encoded =
            serde_json::to_value(HostEventWire::RecentFoldersChanged(folders.clone())).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "kind": "recent_folders_changed",
                "payload": [
                    { "path": "sworm://host/repo", "opened_at": "2026-01-02T03:04:05+00:00" },
                    { "path": "/local/repo", "opened_at": "2026-01-01T03:04:05+00:00" },
                ],
            })
        );
        let HostEventWire::RecentFoldersChanged(decoded) = serde_json::from_value(encoded).unwrap()
        else {
            panic!("decoded event kind changed");
        };
        assert_eq!(decoded, folders);
    }

    #[test]
    fn named_extractor_rejects_another_operation() {
        let error = Reply::FileRead(crate::files::FileContent {
            content: "contents".to_owned(),
            version: "0".to_owned(),
        })
        .files_read_dir()
        .unwrap_err();

        assert_eq!(
            error,
            WireError::Internal {
                message: "unexpected reply variant for files_read_dir".to_owned(),
            }
        );
    }

    #[test]
    fn task_start_requires_explicit_restoration_intent() {
        let request = Request::TasksStart {
            run_id: "saved-run".into(),
            folder_path: "/repo".into(),
            task_id: "build".into(),
            active_file_path: None,
            cols: 80,
            rows: 24,
            attach_only: true,
        };
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(encoded["params"]["attach_only"], true);
        assert!(matches!(
            serde_json::from_value::<Request>(encoded.clone()).unwrap(),
            Request::TasksStart {
                attach_only: true,
                ..
            }
        ));
        let mut old_request = encoded;
        old_request["params"]
            .as_object_mut()
            .unwrap()
            .remove("attach_only");
        assert!(serde_json::from_value::<Request>(old_request).is_err());
    }

    #[test]
    fn open_rpc_flattens_the_typed_request() {
        let open = Open::Rpc(Request::FileRead {
            project_path: "/repo".to_owned(),
            file_path: "src/main.rs".to_owned(),
        });
        let encoded = serde_json::to_value(&open).unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "kind": "rpc",
                "method": "file_read",
                "params": {
                    "project_path": "/repo",
                    "file_path": "src/main.rs",
                },
            })
        );
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<Open>(encoded.clone()).unwrap()).unwrap(),
            encoded
        );
        let Open::Rpc(request) = &open else {
            unreachable!("constructed as Open::Rpc");
        };
        assert_eq!(
            serde_json::to_value(OpenRpc::new(request)).unwrap(),
            encoded,
            "the borrowed envelope the client writes must match the owned frame"
        );
    }

    #[test]
    fn open_workbench_rpc_borrowed_envelope_matches_owned_frame() {
        let request = Request::WorkbenchList {};
        let open = Open::WorkbenchRpc {
            workbench: "wb-1".to_owned(),
            request: request.clone(),
        };
        let encoded = serde_json::to_value(&open).unwrap();
        assert_eq!(
            serde_json::to_value(OpenWorkbenchRpc::new("wb-1", &request)).unwrap(),
            encoded
        );
        assert!(matches!(
            serde_json::from_value::<Open>(encoded).unwrap(),
            Open::WorkbenchRpc { workbench, request: Request::WorkbenchList {} } if workbench == "wb-1"
        ));
    }
}
