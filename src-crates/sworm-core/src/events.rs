use std::path::PathBuf;
use std::sync::Arc;
use sworm_protocol::files::FilesChangedEvent;
use sworm_protocol::git::GitChangedEvent;
use sworm_protocol::rpc::RecentFolder;
use sworm_protocol::settings::SettingsChangedEvent;

/// Delivery completes synchronously. Sinks must not re-enter the emitting
/// service or wait for asynchronous delivery while its state is locked.
pub type EventSink<T> = Arc<dyn Fn(T) -> Result<(), String> + Send + Sync + 'static>;

/// In-process host notifications; desktop audiences and IPC encoding stay in
/// the adapter. This is not a network protocol envelope.
pub enum HostEvent {
    FilesChanged(FilesChangedEvent),
    GitChanged(GitChangedEvent),
    SettingsChanged(SettingsChangedEvent),
    RecentFoldersChanged(Vec<RecentFolder>),
    TasksChanged(String),
    NixChanged(String),
    IssuesChanged(String),
    FileMoved {
        folder_path: PathBuf,
        old_path: PathBuf,
        new_path: PathBuf,
        replace_destination: bool,
    },
    FileDeleted(PathBuf),
    RemoteStatus {
        server: String,
        connected: bool,
        last_error: Option<String>,
        state: String,
    },
    RemoteRunStatus {
        run_id: String,
        state: String,
    },
    /// `server`'s workbench registry changed; the desktop re-lists it.
    RemoteWorkbenchesChanged {
        server: String,
    },
}
