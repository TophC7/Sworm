use serde::{Deserialize, Serialize};

/// Protocol version selected during the QUIC TLS handshake.
///
/// Version 2 adds the frame tag byte. It intentionally does not negotiate
/// with version 1 because the two frame headers are not wire-compatible.
pub const ALPN: &[u8] = b"sworm/2";
pub const DEFAULT_SERVER_PORT: u16 = 7420;
/// Maximum encoded `Open` frame body accepted before a connection is paired,
/// and the ceiling every non-RPC open is written with: pairing metadata,
/// stream ids, and cursors are tiny, so an unauthenticated peer can never
/// make the daemon allocate more than this.
pub const MAX_REQUEST_FRAME_BYTES: usize = 64 * 1024;
/// Maximum JSON or raw frame body.
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;
/// Bound whole-file reads well below the response frame ceiling.
pub const MAX_REMOTE_FILE_BYTES: usize = 16 * 1024 * 1024;
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
/// - `input_folder_path`: the optional `folder_path` inside the `input` argument.
/// - `input_path`: mandatory folder path in the `input` argument.
/// - `server`: no path at all; the caller passes the server explicitly.
/// - `run`: keyed by a registered run id.
/// - `lsp`: keyed by a registered LSP session id.
/// - `none`: connection-level, never routed.
#[macro_export]
macro_rules! sworm_rpc_ops {
    ($callback:ident) => {
        $callback! {
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
            DiffGetFiles => diff_get_files(
                path: String,
                source: $crate::file_diff::DiffSource,
            ) -> Vec<$crate::file_diff::FileDiff>;
            #[route(path)]
            DiffGetWorkingIndex => diff_get_working_index(
                path: String,
                staged: bool,
            ) -> Vec<$crate::file_diff::FileDiff>;
            #[route(path)]
            DiffGetWorkingFile => diff_get_working_file(
                path: String,
                file_path: String,
                status: $crate::file_diff::GitStatus,
                staged: bool,
            ) -> $crate::git::DiffFileContent;
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
            GitStageAll => git_stage_all(
                path: String,
            ) -> ();
            #[route(path)]
            GitStageFiles => git_stage_files(
                path: String,
                files: Vec<String>,
            ) -> ();
            #[route(path)]
            GitUnstageAll => git_unstage_all(
                path: String,
            ) -> ();
            #[route(path)]
            GitUnstageFiles => git_unstage_files(
                path: String,
                files: Vec<String>,
            ) -> ();
            #[route(path)]
            GitDiscardAll => git_discard_all(
                path: String,
            ) -> ();
            #[route(path)]
            GitDiscardFiles => git_discard_files(
                path: String,
                files: Vec<String>,
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
            ) -> String;
            #[route(path)]
            GitUndoLastCommit => git_undo_last_commit(
                path: String,
            ) -> String;
            #[route(path)]
            GitPush => git_push(
                path: String,
            ) -> ();
            #[route(path)]
            GitPushForceWithLease => git_push_force_with_lease(
                path: String,
            ) -> ();
            #[route(path)]
            GitPull => git_pull(
                path: String,
            ) -> ();
            #[route(path)]
            GitFetch => git_fetch(
                path: String,
            ) -> ();
            #[route(path)]
            GitStashAll => git_stash_all(
                path: String,
                message: Option<String>,
            ) -> ();
            #[route(path)]
            GitStashCount => git_stash_count(
                path: String,
            ) -> usize;
            #[route(path)]
            GitStashList => git_stash_list(
                path: String,
            ) -> Vec<$crate::git::StashEntry>;
            #[route(path)]
            GitStashPop => git_stash_pop(
                path: String,
                index: usize,
            ) -> ();
            #[route(path)]
            GitStashDrop => git_stash_drop(
                path: String,
                index: usize,
            ) -> ();
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
            GitBranchStatus => git_branch_status(
                path: String,
            ) -> $crate::branch::BranchOpState;
            #[route(path)]
            GitDiffBranchAgainstHead => git_diff_branch_against_head(
                path: String,
                branch: String,
            ) -> Vec<$crate::file_diff::FileDiff>;
            #[route(path)]
            GitCheckoutBranch => git_checkout_branch(
                path: String,
                name: String,
            ) -> ();
            #[route(path)]
            GitCheckoutRemoteAsLocal => git_checkout_remote_as_local(
                path: String,
                remote_name: String,
                local_name: String,
            ) -> ();
            #[route(path)]
            GitCreateBranch => git_create_branch(
                path: String,
                name: String,
                base: String,
                checkout: bool,
            ) -> ();
            #[route(path)]
            GitRenameBranch => git_rename_branch(
                path: String,
                old_name: String,
                new_name: String,
            ) -> ();
            #[route(path)]
            GitDeleteBranch => git_delete_branch(
                path: String,
                name: String,
                force: bool,
            ) -> ();
            #[route(path)]
            GitDeleteRemoteBranch => git_delete_remote_branch(
                path: String,
                remote: String,
                name: String,
            ) -> ();
            #[route(path)]
            GitSetUpstream => git_set_upstream(
                path: String,
                branch: String,
                upstream: String,
            ) -> ();
            #[route(path)]
            GitFastForwardBranch => git_fast_forward_branch(
                path: String,
                name: String,
            ) -> ();
            #[route(path)]
            GitMergeIntoCurrent => git_merge_into_current(
                path: String,
                source: String,
                no_ff: bool,
            ) -> ();
            #[route(path)]
            GitRebaseCurrentOnto => git_rebase_current_onto(
                path: String,
                target: String,
            ) -> ();
            #[route(path)]
            GitRebaseContinue => git_rebase_continue(
                path: String,
            ) -> ();
            #[route(path)]
            GitRebaseSkip => git_rebase_skip(
                path: String,
            ) -> ();
            #[route(path)]
            GitRebaseAbort => git_rebase_abort(
                path: String,
            ) -> ();
            #[route(path)]
            GitMergeAbort => git_merge_abort(
                path: String,
            ) -> ();
            #[route(folder_path)]
            IssuesList => issues_list(
                folder_path: String,
                filters: $crate::issues::IssueListFilters,
            ) -> Vec<$crate::issues::Issue>;
            #[route(folder_path)]
            IssuesReady => issues_ready(
                folder_path: String,
                limit: Option<i64>,
                filters: Option<$crate::issues::IssueReadyFilters>,
            ) -> Vec<$crate::issues::Issue>;
            #[route(folder_path)]
            IssuesSearch => issues_search(
                folder_path: String,
                query: String,
                filters: $crate::issues::IssueSearchFilters,
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
            #[route(input_folder_path)]
            SettingsGetEffective => settings_get_effective(
                input: $crate::settings::EffectiveSettingsInput,
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
                config: $crate::settings::SaveProviderConfigInput,
            ) -> $crate::settings::ProviderConfigRecord;
            #[route(input_path)]
            SettingsOpenFolderFile => settings_open_folder_file(
                input: $crate::settings::FolderSettingsFileInput,
            ) -> $crate::settings::SettingsFileResult;
            #[route(opt_folder_path)]
            LspListServers => lsp_list_servers(
                folder_path: Option<String>,
            ) -> Vec<$crate::lsp::LspServerSettingsEntry>;
            #[route(server)]
            LspSetServerConfig => lsp_set_server_config(
                config: $crate::lsp::SaveLspServerConfigInput,
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
            #[route(none)]
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

        impl Request {
            pub fn method(&self) -> &'static str {
                match self {
                    $(
                        Self::$variant { .. } => stringify!($method),
                    )*
                }
            }
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
    Events,
    Pty { run_id: String, cursor: PtyCursor },
    Lsp { session_id: String },
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

/// Daemon-to-desktop frame on the host-events stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HostEventFrame(pub HostEventWire);

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
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn operation_table_names_every_method_once() {
        macro_rules! collect_methods {
            (
                $(
                    #[route($route:ident)]
                    $variant:ident => $method:ident(
                        $($argument:ident: $argument_type:ty),* $(,)?
                    ) -> $return_type:ty;
                )*
            ) => {
                vec![$(stringify!($method)),*]
            };
        }

        let methods: Vec<&str> = sworm_rpc_ops!(collect_methods);
        let unique: std::collections::BTreeSet<&str> = methods.iter().copied().collect();
        assert_eq!(
            unique.len(),
            methods.len(),
            "two operations share a method name, so reply extraction is ambiguous"
        );
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
}
