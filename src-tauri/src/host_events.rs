use crate::services::windows::WindowCoordinatorService;
use serde::Serialize;
use serde_json::json;
use std::sync::Arc;
use sworm_core::events::{EventSink, HostEvent};
use sworm_protocol::files::FILES_CHANGED_EVENT;
use sworm_protocol::git::GIT_CHANGED_EVENT;
use sworm_protocol::issues::ISSUES_CHANGED_EVENT;
use sworm_protocol::nix_env::NIX_CHANGED_EVENT;
use sworm_protocol::settings::SETTINGS_CHANGED_EVENT;
use sworm_protocol::task::TASKS_CHANGED_EVENT;
use tauri::Emitter;

pub fn channel_sink<T: Serialize + Send + Sync + 'static>(
    channel: tauri::ipc::Channel<T>,
) -> EventSink<T> {
    Arc::new(move |payload| channel.send(payload).map_err(|error| error.to_string()))
}

pub fn host_event_sink(
    app: tauri::AppHandle,
    windows: Arc<WindowCoordinatorService>,
) -> EventSink<HostEvent> {
    Arc::new(move |event| {
        let result = match event {
            HostEvent::RemoteStatus { server, connected, last_error, state } =>
                app.emit("remote-status", json!({ "server": server, "connected": connected, "last_error": last_error, "state": state })),
            HostEvent::RemoteWorkbenchesChanged { server } =>
                app.emit("workbenches-changed", json!({ "server": server })),
            HostEvent::RemoteRunStatus { run_id, state } =>
                app.emit("remote-run-status", json!({ "runId": run_id, "state": state })),
            HostEvent::FilesChanged(payload) => app.emit(FILES_CHANGED_EVENT, payload),
            HostEvent::GitChanged(payload) => app.emit(GIT_CHANGED_EVENT, payload),
            HostEvent::SettingsChanged(payload) => app.emit(SETTINGS_CHANGED_EVENT, payload),
            HostEvent::RecentFoldersChanged(folders) => {
                app.emit("recent-folders-changed", folders)
            }
            HostEvent::TasksChanged(folder) => app.emit(TASKS_CHANGED_EVENT, folder),
            HostEvent::NixChanged(folder) => {
                app.emit(NIX_CHANGED_EVENT, json!({ "folderPath": folder }))
            }
            HostEvent::IssuesChanged(folder) => {
                app.emit(ISSUES_CHANGED_EVENT, json!({ "folderPath": folder }))
            }
            HostEvent::FileMoved {
                folder_path,
                old_path,
                new_path,
                replace_destination,
            } => {
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
            HostEvent::FileDeleted(path) => {
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
