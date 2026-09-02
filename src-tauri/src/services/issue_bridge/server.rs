//! Listener task and per-connection NDJSON read/write loop.

use super::dispatch::handle_request;
use super::protocol::{BridgeRequest, BridgeResponse};
use crate::services::issues::IssueService;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::oneshot;

/// Accept loop for one folder's bridge socket. Owners unlink the socket
/// synchronously when they send `shutdown_rx`, so this task must not
/// touch the path then: a replacement listener may already be bound to it.
/// Only the self-initiated exit (accept failure) cleans up here.
pub(super) async fn run_bridge(
    listener: UnixListener,
    issues: Arc<IssueService>,
    project_path: PathBuf,
    token: String,
    mut shutdown_rx: oneshot::Receiver<()>,
    socket_path: PathBuf,
) {
    loop {
        tokio::select! {
            _ = &mut shutdown_rx => return,
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _)) => {
                        let issues = Arc::clone(&issues);
                        let project_path = project_path.clone();
                        let token = token.clone();
                        tauri::async_runtime::spawn(async move {
                            handle_client(stream, issues, project_path, token).await;
                        });
                    }
                    Err(error) => {
                        tracing::warn!("Issue bridge accept failed: {}", error);
                        let _ = std::fs::remove_file(socket_path);
                        return;
                    }
                }
            }
        }
    }
}

async fn handle_client(
    stream: UnixStream,
    issues: Arc<IssueService>,
    project_path: PathBuf,
    token: String,
) {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let response = match serde_json::from_str::<BridgeRequest>(&line) {
            Ok(request) => handle_request(request, &issues, &project_path, &token),
            Err(error) => BridgeResponse::error(None, "bad_request", &error.to_string()),
        };
        let Ok(payload) = serde_json::to_vec(&response) else {
            break;
        };
        if writer.write_all(&payload).await.is_err() || writer.write_all(b"\n").await.is_err() {
            break;
        }
    }
}
