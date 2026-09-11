use serde::{Deserialize, Serialize};

/// Protocol version selected during the QUIC TLS handshake.
///
/// Version 2 adds the frame tag byte. It intentionally does not negotiate
/// with version 1 because the two frame headers are not wire-compatible.
pub const ALPN: &[u8] = b"sworm/2";
pub const DEFAULT_SERVER_PORT: u16 = 7420;
/// Maximum encoded `Open` frame body. Requests contain paths and pairing
/// metadata, never file bodies.
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
/// Route keys are `project_path`, `path`, `folder_path`, `run`, or `none`.
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
            ) -> String;
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
        #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Open {
    Rpc(Request),
    Events,
    Pty { run_id: String, cursor: PtyCursor },
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
/// Local mutation bookkeeping events (`FileMoved` and `FileDeleted`) are
/// intentionally absent.
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
    Database { message: String },
    Pty { message: String },
    Io { message: String },
    NotFound { message: String },
    InvalidArgument { message: String },
    Internal { message: String },
    BranchUnmerged { branch: String, message: String },
    DirtyWorktree { message: String },
    Unauthorized { message: String },
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
    fn operation_table_generates_request_methods() {
        macro_rules! assert_methods {
            (
                $(
                    #[route($route:ident)]
                    $variant:ident => $method:ident(
                        $($argument:ident: $argument_type:ty),* $(,)?
                    ) -> $return_type:ty;
                )*
            ) => {
                $(
                    let request = Request::$variant {
                        $($argument: <$argument_type>::default()),*
                    };
                    assert_eq!(request.method(), stringify!($method));
                )*
            };
        }

        sworm_rpc_ops!(assert_methods);
    }

    #[test]
    fn named_extractor_rejects_another_operation() {
        let error = Reply::FileRead("contents".to_owned())
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
        assert_eq!(serde_json::from_value::<Open>(encoded).unwrap(), open);
    }
}
