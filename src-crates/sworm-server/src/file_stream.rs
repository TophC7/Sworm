use crate::stream::{StreamReader, StreamWriter};
use std::sync::Arc;
use sworm_core::Host;
use sworm_protocol::rpc::{FileReadDown, WireError, MAX_FILE_CHUNK_BYTES};
use tokio::{sync::watch, task::JoinSet};

pub(crate) async fn run(
    host: Arc<Host>,
    project_path: String,
    file_path: String,
    version: String,
    mut send: StreamWriter,
    mut recv: StreamReader,
    shutdown: watch::Receiver<bool>,
) {
    // FileRead has no application input. Its only reader observes peer closure
    // (or rejects unsolicited frames), then signals the writer. Join it before
    // returning: no upgraded WebSocket reader may outlive its file stream.
    let mut readers = JoinSet::new();
    if recv.is_web() {
        readers.spawn(async move {
            if let Err(error) = recv.observe_close().await {
                tracing::debug!(%error, "FileRead reader stopped");
            }
        });
    }
    produce(host, project_path, file_path, version, &mut send, shutdown).await;
    readers.abort_all();
    while readers.join_next().await.is_some() {}
}

async fn produce(
    host: Arc<Host>,
    project_path: String,
    file_path: String,
    version: String,
    send: &mut StreamWriter,
    mut shutdown: watch::Receiver<bool>,
) {
    let result = tokio::select! {
        biased;
        _ = shutdown.changed() => return,
        _ = send.stopped() => return,
        result = host.file_open_read_stream(project_path, file_path, version) => result,
    };
    let terminal = match result {
        Err(error) => FileReadDown::Error {
            error: error.into(),
        },
        Ok(mut reader) => {
            // One chunk buffer for the whole stream, moved into each blocking read and back.
            let mut chunk = Vec::with_capacity(MAX_FILE_CHUNK_BYTES);
            loop {
                // At most one chunk is in flight. Dropping this future can leave only
                // that bounded disk read running, never a detached full-file producer.
                let read = tokio::task::spawn_blocking(move || {
                    chunk.clear();
                    let count = reader.read_chunk(&mut chunk);
                    (reader, chunk, count)
                });
                let result = tokio::select! {
                    biased;
                    _ = shutdown.changed() => return,
                    _ = send.stopped() => return,
                    result = read => result,
                };
                let (next_reader, next_chunk, count) = match result {
                    Ok(result) => result,
                    Err(error) => {
                        break FileReadDown::Error {
                            error: WireError::Internal {
                                message: error.to_string(),
                            },
                        }
                    }
                };
                reader = next_reader;
                chunk = next_chunk;
                match count {
                    Err(error) => {
                        break FileReadDown::Error {
                            error: error.into(),
                        }
                    }
                    Ok(0) => {
                        break FileReadDown::Complete {
                            version: reader.version(),
                        }
                    }
                    Ok(_) => {
                        tokio::select! {
                            biased;
                            _ = shutdown.changed() => return,
                            result = send.write_raw(&chunk) => if result.is_err() { return; },
                        }
                    }
                }
            }
        }
    };
    tokio::select! {
        biased;
        _ = shutdown.changed() => {},
        _ = async {
            if send.write_json(&terminal).await.is_ok() {
                let _ = send.finish().await;
            }
        } => {},
    }
}
