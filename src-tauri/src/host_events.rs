use crate::windows::WindowCoordinatorService;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use sworm_core::events::{EventSink, HostEvent};
use tauri::Emitter;

pub enum DesktopEvent {
    Host(HostEvent),
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
    WorkbenchesChanged {
        server: String,
    },
}

pub fn channel_sink<T: Serialize + Send + Sync + 'static>(
    channel: tauri::ipc::Channel<T>,
) -> EventSink<T> {
    Arc::new(move |payload| channel.send(payload).map_err(|error| error.to_string()))
}

pub fn desktop_event_sink(
    app: tauri::AppHandle,
    windows: Arc<WindowCoordinatorService>,
) -> EventSink<DesktopEvent> {
    Arc::new(move |event| {
        let result = match event {
            DesktopEvent::RemoteStatus { server, connected, last_error, state } =>
                app.emit("remote-status", json!({ "server": server, "connected": connected, "last_error": last_error, "state": state })),
            DesktopEvent::WorkbenchesChanged { server } =>
                app.emit("workbenches-changed", json!({ "server": server })),
            DesktopEvent::RemoteRunStatus { run_id, state } =>
                app.emit("remote-run-status", json!({ "runId": run_id, "state": state })),
            DesktopEvent::Host(HostEvent::FilesChanged(payload)) => app.emit("files-changed", payload),
            DesktopEvent::Host(HostEvent::GitChanged(payload)) => app.emit("git-changed", payload),
            DesktopEvent::Host(HostEvent::SettingsChanged(payload)) => app.emit("settings-changed", payload),
            DesktopEvent::Host(HostEvent::RecentFoldersChanged(folders)) => {
                app.emit("recent-folders-changed", folders)
            }
            DesktopEvent::Host(HostEvent::TasksChanged(folder)) => app.emit("tasks-changed", folder),
            DesktopEvent::Host(HostEvent::NixChanged(folder)) => {
                app.emit("nix-changed", json!({ "folderPath": folder }))
            }
            DesktopEvent::Host(HostEvent::IssuesChanged(folder)) => {
                app.emit("issues-changed", json!({ "folderPath": folder }))
            }
            DesktopEvent::Host(HostEvent::FileMoved {
                folder_path,
                old_path,
                new_path,
                replace_destination,
            }) => {
                if replace_destination && windows.release_claims_under(&new_path) > 0 {
                    let _ = app.emit(
                        "file-deleted",
                        json!({ "filePath": new_path.to_string_lossy() }),
                    );
                }
                windows.rename_claims_under(&old_path, &new_path);
                app.emit(
                    "file-path-changed",
                    json!({
                        "oldPath": old_path.to_string_lossy(),
                        "newPath": new_path.to_string_lossy(),
                        "folderPath": folder_path.to_string_lossy(),
                    }),
                )
            }
            DesktopEvent::Host(HostEvent::FileDeleted(path)) => {
                windows.release_claims_under(&path);
                app.emit(
                    "file-deleted",
                    json!({ "filePath": path.to_string_lossy() }),
                )
            }
        };
        result.map_err(|error| error.to_string())
    })
}
