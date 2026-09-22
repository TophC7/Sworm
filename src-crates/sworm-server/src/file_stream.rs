use std::{sync::Arc, time::Duration};
use sworm_core::Host;
use sworm_protocol::rpc::{FileReadDown, WireError, MAX_FILE_CHUNK_BYTES};
use sworm_remote::wire::{write_frame, write_raw_frame};
use tokio::{sync::watch, time::timeout};

pub(crate) async fn run(
    host: Arc<Host>,
    project_path: String,
    file_path: String,
    version: String,
    mut send: quinn::SendStream,
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
                        let written = tokio::select! {
                            biased;
                            _ = shutdown.changed() => return,
                            result = timeout(Duration::from_secs(30), write_raw_frame(&mut send, &chunk)) => result,
                        };
                        if !matches!(written, Ok(Ok(()))) {
                            return;
                        }
                    }
                }
            }
        }
    };
    if matches!(
        timeout(Duration::from_secs(30), write_frame(&mut send, &terminal)).await,
        Ok(Ok(()))
    ) {
        let _ = send.finish();
    }
}
