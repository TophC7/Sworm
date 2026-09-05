//! Per-project Unix-domain-socket bridge that exposes
//! [`IssueService`] over a small NDJSON RPC protocol so out-of-process
//! agent clients (the `sworm-issues` OMP extension) can read and mutate
//! issue memory without sharing the SQLite handle.
//!
//! The bridge is started lazily by
//! [`crate::commands::sessions`] when a session is launched for a
//! provider that should see issue memory, and torn down when
//! [`IssueBridgeService`] is dropped or [`Self::stop`] is called.
//!
//! Submodules:
//! - [`protocol`] — wire format and error classification
//! - [`server`] — accept loop and per-connection NDJSON I/O
//! - [`dispatch`] — method router into [`IssueService`]
//! - [`socket`] — socket path resolution and private-dir creation

mod dispatch;
mod protocol;
mod server;
mod socket;

use crate::services::issues::IssueService;
use parking_lot::Mutex;
use protocol::PROTOCOL_VERSION;
use serde::Serialize;
use server::run_bridge;
use socket::{create_private_dir, socket_path_for};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::oneshot;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Per-project bridge coordinates injected into agent child processes
/// as `SWORM_ISSUES_*` environment variables. Returned by
/// [`IssueBridgeService::ensure_running`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueBridgeInfo {
    pub project_path: String,
    pub socket_path: String,
    pub token: String,
    pub protocol_version: u32,
}

struct BridgeRuntime {
    info: IssueBridgeInfo,
    shutdown: Option<oneshot::Sender<()>>,
}

impl BridgeRuntime {
    /// Signal the accept loop to exit and unlink the socket file. This is
    /// the only unlink on the shutdown path (see `run_bridge`).
    fn teardown(mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        let _ = std::fs::remove_file(&self.info.socket_path);
    }
}

/// Per-project Unix-socket listeners that expose [`IssueService`] to
/// agent child processes via NDJSON RPC.
pub struct IssueBridgeService {
    issues: Arc<IssueService>,
    app: Option<tauri::AppHandle>,
    runtimes: Mutex<HashMap<String, Arc<Mutex<Option<BridgeRuntime>>>>>,
}

impl IssueBridgeService {
    /// Build a fresh bridge service backed by the shared
    /// [`IssueService`] handle. No sockets are created until
    /// [`Self::ensure_running`] is called.
    #[cfg(test)]
    pub fn new(issues: Arc<IssueService>) -> Self {
        Self {
            issues,
            app: None,
            runtimes: Mutex::new(HashMap::new()),
        }
    }

    pub fn new_with_app(issues: Arc<IssueService>, app: tauri::AppHandle) -> Self {
        Self {
            issues,
            app: Some(app),
            runtimes: Mutex::new(HashMap::new()),
        }
    }

    /// Idempotent: returns the bridge coordinates for the folder at
    /// `project_path` (keyed by its canonical string form), starting a
    /// new listener task if one isn't already running (or if the
    /// previous socket file disappeared).
    pub fn ensure_running(&self, project_path: &Path) -> Result<IssueBridgeInfo, String> {
        let key = project_path.to_string_lossy();
        let slot = {
            let mut runtimes = self.runtimes.lock();
            Arc::clone(runtimes.entry(key.to_string()).or_default())
        };
        // Serialize initialization per project without blocking other projects.
        let mut guard = slot.lock();
        if let Some(existing) = guard.as_ref() {
            if Path::new(&existing.info.socket_path).exists() {
                return Ok(existing.info.clone());
            }
        }
        if let Some(stale) = guard.take() {
            stale.teardown();
        }
        let socket_path = socket_path_for(project_path)?;
        if socket_path.exists() {
            std::fs::remove_file(&socket_path).map_err(|e| {
                format!(
                    "Failed to remove stale issue bridge socket {}: {}",
                    socket_path.display(),
                    e
                )
            })?;
        }
        if let Some(parent) = socket_path.parent() {
            create_private_dir(parent).map_err(|e| {
                format!(
                    "Failed to create issue bridge dir {}: {}",
                    parent.display(),
                    e
                )
            })?;
        }

        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            format!(
                "Failed to bind issue bridge socket {}: {}",
                socket_path.display(),
                e
            )
        })?;
        #[cfg(unix)]
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600)).map_err(
            |e| {
                format!(
                    "Failed to set issue bridge socket permissions {}: {}",
                    socket_path.display(),
                    e
                )
            },
        )?;

        let token = Uuid::new_v4().to_string();
        let key = key.into_owned();
        let info = IssueBridgeInfo {
            project_path: key,
            socket_path: socket_path.to_string_lossy().into_owned(),
            token: token.clone(),
            protocol_version: PROTOCOL_VERSION,
        };
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let issues = Arc::clone(&self.issues);
        let app = self.app.clone();
        let project_path_owned = project_path.to_path_buf();
        let socket_for_task = socket_path.clone();
        let token_for_task = token.clone();

        tauri::async_runtime::spawn(async move {
            run_bridge(
                listener,
                issues,
                app,
                project_path_owned,
                token_for_task,
                shutdown_rx,
                socket_for_task,
            )
            .await;
        });

        // Publish while still holding the project's initialization lock.
        *guard = Some(BridgeRuntime {
            info: info.clone(),
            shutdown: Some(shutdown_tx),
        });

        Ok(info)
    }

    /// Tear down the bridge for the folder at `project_path`: stop the
    /// listener task and unlink its socket. No-op when no bridge is
    /// running for that folder.
    pub fn stop(&self, project_path: &Path) {
        let key = project_path.to_string_lossy();
        let slot = self.runtimes.lock().remove(key.as_ref());
        if let Some(slot) = slot {
            let mut guard = slot.lock();
            if let Some(runtime) = guard.take() {
                runtime.teardown();
            }
        }
    }
}

impl Drop for IssueBridgeService {
    fn drop(&mut self) {
        let runtimes = std::mem::take(&mut *self.runtimes.lock());
        for (_, slot) in runtimes {
            let mut guard = slot.lock();
            if let Some(runtime) = guard.take() {
                runtime.teardown();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    fn temp_project(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("sworm-bridge-test-{}-{}", name, Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    async fn rpc(info: &IssueBridgeInfo, method: &str, params: Value) -> Value {
        let mut stream = UnixStream::connect(&info.socket_path).await.unwrap();
        let request = json!({
            "id": method,
            "token": info.token.as_str(),
            "method": method,
            "params": params,
        });
        stream
            .write_all(format!("{}\n", request).as_bytes())
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            response.get("ok").and_then(Value::as_bool),
            Some(true),
            "response was: {line}"
        );
        response.get("result").cloned().unwrap_or(Value::Null)
    }

    #[tokio::test]
    async fn bridge_requires_token_and_serves_info() {
        let issues = Arc::new(IssueService::new());
        let bridge = IssueBridgeService::new(issues);
        let project = temp_project("info");
        let info = bridge.ensure_running(&project).unwrap();

        let mut stream = UnixStream::connect(&info.socket_path).await.unwrap();
        stream
            .write_all(
                b"{\"id\":\"bad\",\"token\":\"nope\",\"method\":\"bridge.info\",\"params\":{}}\n",
            )
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("unauthorized"));

        let mut stream = UnixStream::connect(&info.socket_path).await.unwrap();
        let request = format!(
            "{{\"id\":\"ok\",\"token\":\"{}\",\"method\":\"bridge.info\",\"params\":{{}}}}\n",
            info.token
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("issues.v1"));
        drop(bridge);
        let _ = std::fs::remove_dir_all(project);
    }

    #[tokio::test]
    async fn bridge_issue_methods_use_issue_service() {
        let issues = Arc::new(IssueService::new());
        let bridge = IssueBridgeService::new(Arc::clone(&issues));
        let project = temp_project("methods");
        let info = bridge.ensure_running(&project).unwrap();

        // Issues require an epic, so seed one directly via the service.
        let epic = issues
            .create_epic(
                &project,
                crate::models::issues::IssueEpicCreateInput {
                    title: "Bridge epic".into(),
                    description: None,
                    status: None,
                    priority: None,
                    actor: None,
                },
            )
            .unwrap();

        let mut stream = UnixStream::connect(&info.socket_path).await.unwrap();
        let request = format!(
            "{{\"id\":\"create\",\"token\":\"{}\",\"method\":\"issue.create\",\"params\":{{\"title\":\"Bridge issue\",\"epicId\":\"{}\"}}}}\n",
            info.token, epic.id
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("ISSUE-1"), "response was: {line}");

        let mut stream = UnixStream::connect(&info.socket_path).await.unwrap();
        // No epic id; should now classify as invalid_argument rather
        // than the prior catch-all `domain_error`.
        let bad = format!(
            "{{\"id\":\"bad\",\"token\":\"{}\",\"method\":\"issue.create\",\"params\":{{\"title\":\"Orphan\"}}}}\n",
            info.token
        );
        stream.write_all(bad.as_bytes()).await.unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("invalid_argument"), "response was: {line}");

        drop(bridge);
        let _ = std::fs::remove_dir_all(project);
    }

    #[tokio::test]
    async fn ensure_running_recovers_after_socket_loss() {
        let issues = Arc::new(IssueService::new());
        let bridge = IssueBridgeService::new(issues);
        let project = temp_project("recover");

        let first = bridge.ensure_running(&project).unwrap();
        assert!(Path::new(&first.socket_path).exists());

        // socket file vanishes underneath us (tmpfs flush, manual rm)
        // -> ensure_running rebinds rather than handing back a stale entry
        std::fs::remove_file(&first.socket_path).unwrap();
        let second = bridge.ensure_running(&project).unwrap();
        assert_ne!(first.token, second.token);
        assert!(Path::new(&second.socket_path).exists());

        drop(bridge);
        let _ = std::fs::remove_dir_all(project);
    }

    #[tokio::test]
    async fn bridge_exposes_full_issue_surface() {
        let issues = Arc::new(IssueService::new());
        let bridge = IssueBridgeService::new(issues);
        let project = temp_project("full-surface");
        let info = bridge.ensure_running(&project).unwrap();

        let bridge_info = rpc(&info, "bridge.info", json!({})).await;
        assert!(bridge_info
            .get("methods")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .any(|method| method == "epic.create"));

        let epic = rpc(
            &info,
            "epic.create",
            json!({"title":"Full epic","description":"Bridge parity"}),
        )
        .await;
        let epic_id = epic.get("id").and_then(Value::as_str).unwrap();
        assert_eq!(epic_id, "EPIC-1");
        assert_eq!(
            rpc(&info, "epic.list", json!({}))
                .await
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            rpc(&info, "epic.show", json!({"epicId": epic_id}))
                .await
                .get("id")
                .and_then(Value::as_str),
            Some(epic_id)
        );
        assert_eq!(
            rpc(
                &info,
                "epic.update",
                json!({"epicId": epic_id, "patch": {"priority": 1}}),
            )
            .await
            .get("priority")
            .and_then(Value::as_i64),
            Some(1)
        );

        let issue_a = rpc(
            &info,
            "issue.create",
            json!({"title":"A","epicId": epic_id,"tags":["spec:fresh"],"contextJson":"{\"kind\":\"spec\"}"}),
        )
        .await;
        let issue_b = rpc(
            &info,
            "issue.create",
            json!({"title":"B","epicId": epic_id,"priority": 3}),
        )
        .await;
        let issue_a_id = issue_a.get("id").and_then(Value::as_str).unwrap();
        let issue_b_id = issue_b.get("id").and_then(Value::as_str).unwrap();
        assert_eq!(issue_a_id, "ISSUE-1");
        assert_eq!(issue_b_id, "ISSUE-2");

        rpc(
            &info,
            "dependency.add",
            json!({"issueId": issue_b_id,"dependsOnIssueId": issue_a_id}),
        )
        .await;
        assert_eq!(
            rpc(&info, "dependency.list", json!({"issueId": issue_b_id}))
                .await
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            rpc(
                &info,
                "issue.ready",
                json!({"filters":{"epicId": epic_id,"limit": 10}}),
            )
            .await
            .as_array()
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            rpc(
                &info,
                "issue.list",
                json!({"filters":{"epicId": epic_id,"limit": 10}}),
            )
            .await
            .as_array()
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            rpc(
                &info,
                "issue.search",
                json!({"query":"A","filters":{"epicId": epic_id}}),
            )
            .await
            .as_array()
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            rpc(&info, "issue.show", json!({"issueId": issue_a_id}))
                .await
                .get("issue")
                .and_then(|issue| issue.get("id"))
                .and_then(Value::as_str),
            Some(issue_a_id)
        );
        assert_eq!(
            rpc(
                &info,
                "issue.claim",
                json!({"issueId": issue_a_id,"assigneeKind":"agent","assigneeId":"omp"}),
            )
            .await
            .get("status")
            .and_then(Value::as_str),
            Some("in_progress")
        );
        assert_eq!(
            rpc(
                &info,
                "issue.update",
                json!({"issueId": issue_a_id,"patch":{"status":"completed"}}),
            )
            .await
            .get("status")
            .and_then(Value::as_str),
            Some("completed")
        );

        let comment = rpc(
            &info,
            "comment.add",
            json!({"issueId": issue_a_id,"author":"omp","body":"done"}),
        )
        .await;
        let comment_id = comment.get("id").and_then(Value::as_str).unwrap();
        assert_eq!(
            rpc(&info, "comment.list", json!({"issueId": issue_a_id}))
                .await
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            rpc(
                &info,
                "comment.update",
                json!({"commentId": comment_id,"input":{"body":"done done"}}),
            )
            .await
            .get("body")
            .and_then(Value::as_str),
            Some("done done")
        );
        rpc(&info, "comment.delete", json!({"commentId": comment_id})).await;

        rpc(
            &info,
            "dependency.remove",
            json!({"issueId": issue_b_id,"dependsOnIssueId": issue_a_id}),
        )
        .await;
        assert!(
            rpc(&info, "dependency.list", json!({"issueId": issue_b_id}))
                .await
                .as_array()
                .unwrap()
                .is_empty()
        );

        assert!(rpc(&info, "config.list", json!({}))
            .await
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry.get("value") == Some(&json!("ISSUE"))));
        assert_eq!(
            rpc(&info, "config.get", json!({"key":"issue_prefix"}))
                .await
                .get("value")
                .and_then(Value::as_str),
            Some("ISSUE")
        );
        assert_eq!(
            rpc(
                &info,
                "config.set",
                json!({"key":"comment_prefix","value":"NOTE"}),
            )
            .await
            .get("value")
            .and_then(Value::as_str),
            Some("NOTE")
        );

        rpc(&info, "issue.delete", json!({"issueId": issue_b_id})).await;
        rpc(&info, "issue.delete", json!({"issueId": issue_a_id})).await;
        rpc(&info, "epic.delete", json!({"epicId": epic_id})).await;

        drop(bridge);
        let _ = std::fs::remove_dir_all(project);
    }
}
