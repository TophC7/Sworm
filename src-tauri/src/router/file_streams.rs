use super::{
    target::{remote_error, Target},
    RouterInner, WorkspaceRouter,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use sworm_core::errors::ApiError;
use sworm_protocol::rpc::Open;

/// Each streamed assembly holds up to 256 MiB; more than this queue.
const MAX_FILE_STREAMS: usize = 2;

fn append_file_chunk(bytes: &mut Vec<u8>, chunk: &[u8], total: u64) -> Result<(), ApiError> {
    let cap = sworm_protocol::rpc::MAX_STREAM_FILE_BYTES;
    if chunk.len() > cap.saturating_sub(bytes.len())
        || bytes.len() as u64 + chunk.len() as u64 > total
    {
        return Err(ApiError::InvalidArgument(
            "File stream exceeds size limit".into(),
        ));
    }
    bytes.extend_from_slice(chunk);
    Ok(())
}

pub(super) struct FileStreams {
    reads: Mutex<HashMap<(String, String), tokio::sync::watch::Sender<bool>>>,
    permits: tokio::sync::Semaphore,
}

impl FileStreams {
    pub(super) fn new() -> Self {
        Self {
            reads: Mutex::new(HashMap::new()),
            permits: tokio::sync::Semaphore::new(MAX_FILE_STREAMS),
        }
    }

    pub(super) fn release_owner(&self, owner: &str) {
        self.reads.lock().retain(|(window, _), cancel| {
            if window != owner {
                return true;
            }
            let _ = cancel.send(true);
            false
        });
    }

    pub(super) fn cancel(&self, owner: &str, request_id: &str) {
        let key = (owner.to_owned(), request_id.to_owned());
        let mut reads = self.reads.lock();
        // Keep a cancelled tombstone: cancellation may arrive before invocation.
        let cancel = reads
            .entry(key)
            .or_insert_with(|| tokio::sync::watch::channel(false).0);
        cancel.send_replace(true);
    }

    pub(super) async fn read(
        &self,
        inner: &RouterInner,
        owner: &str,
        request_id: &str,
        project_path: String,
        file_path: String,
        version: String,
        size: u64,
        progress: impl Fn(u64, u64) + Send,
    ) -> Result<sworm_protocol::files::FileContent, ApiError> {
        if request_id.is_empty() || request_id.len() > 128 {
            return Err(ApiError::InvalidArgument(
                "Invalid file-read request id".into(),
            ));
        }
        let key = (owner.to_owned(), request_id.to_owned());
        let mut cancelled = {
            let mut reads = self.reads.lock();
            if let Some(existing) = reads.get(&key) {
                let cancelled = *existing.borrow();
                if cancelled {
                    reads.remove(&key);
                    return Err(ApiError::InvalidArgument("File read cancelled".into()));
                }
                return Err(ApiError::InvalidArgument(
                    "File-read request id already in use".into(),
                ));
            }
            let (send, recv) = tokio::sync::watch::channel(false);
            reads.insert(key.clone(), send);
            recv
        };
        struct ReadGuard<'a>(&'a FileStreams, (String, String));
        impl Drop for ReadGuard<'_> {
            fn drop(&mut self) {
                self.0.reads.lock().remove(&self.1);
            }
        }
        let _guard = ReadGuard(self, key);
        let cancellation = cancelled.clone();
        let progress = move |bytes, total| {
            progress(bytes, total);
            // Buffered frames can complete in one poll, before select! polls
            // its cancellation branch again. Stop at each chunk boundary too.
            if *cancellation.borrow() {
                return Err(ApiError::InvalidArgument("File read cancelled".into()));
            }
            Ok(())
        };
        tokio::select! {
            biased;
            _ = cancelled.changed() => Err(ApiError::InvalidArgument("File read cancelled".into())),
            result = self.assemble(inner, project_path, file_path, version, size, progress) => {
                if *cancelled.borrow() {
                    Err(ApiError::InvalidArgument("File read cancelled".into()))
                } else {
                    result
                }
            },
        }
    }

    /// `version` and `size` are the caller's approved stat. The open on the
    /// side holding the file checks that identity once and pins it for every
    /// chunk, so nothing here stats again.
    async fn assemble(
        &self,
        inner: &RouterInner,
        project_path: String,
        file_path: String,
        version: String,
        size: u64,
        progress: impl Fn(u64, u64) -> Result<(), ApiError> + Send,
    ) -> Result<sworm_protocol::files::FileContent, ApiError> {
        use sha2::{Digest, Sha256};
        use sworm_protocol::rpc::{FileReadDown, MAX_FILE_CHUNK_BYTES, MAX_STREAM_FILE_BYTES};
        use sworm_remote::wire::{read_tagged_frame_with_limit, Frame};
        if size > MAX_STREAM_FILE_BYTES as u64 {
            return Err(ApiError::TooLarge {
                size,
                limit: MAX_STREAM_FILE_BYTES as u64,
            });
        }
        // Held until this future ends by any path; waiting here stays
        // cancellable because the caller selects on cancellation.
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?;
        let mut bytes = Vec::with_capacity(size as usize);
        progress(0, size)?;
        let final_version = if let Target::Remote { server, path } = Target::parse(&project_path)? {
            let client = inner.client(server).await?;
            let (mut send, mut recv) = client
                .open_stream(Open::FileRead {
                    project_path: path.into(),
                    file_path,
                    version,
                })
                .await
                .map_err(|error| remote_error(server, error))?;
            let _ = send.finish();
            let mut hash = Sha256::new();
            loop {
                match read_tagged_frame_with_limit::<FileReadDown>(&mut recv, MAX_FILE_CHUNK_BYTES)
                    .await
                    .map_err(|error| remote_error(server, error))?
                {
                    Frame::Raw(chunk) => {
                        append_file_chunk(&mut bytes, &chunk, size)?;
                        hash.update(&chunk);
                        progress(bytes.len() as u64, size)?;
                    }
                    // The daemon's version is trusted only once our own hash agrees.
                    Frame::Json(FileReadDown::Complete { version }) => {
                        if format!("{:x}", hash.finalize()) != version {
                            return Err(ApiError::Remote("File stream hash mismatch".into()));
                        }
                        break version;
                    }
                    Frame::Json(FileReadDown::Error { error }) => return Err(error.into()),
                }
            }
        } else {
            let mut reader = inner
                .local(move |host| host.file_open_read_stream(project_path, file_path, version))
                .await?;
            // Chunks land straight in the presized buffer; the reader hashes
            // exactly the bytes it appends, so its version is this content's.
            loop {
                let (next_reader, next_bytes, count) = tokio::task::spawn_blocking(move || {
                    let count = reader.read_chunk(&mut bytes);
                    (reader, bytes, count)
                })
                .await?;
                reader = next_reader;
                bytes = next_bytes;
                if count? == 0 {
                    break reader.version();
                }
                progress(bytes.len() as u64, size)?;
            }
        };
        if bytes.len() as u64 != size {
            return Err(ApiError::Remote("File stream size mismatch".into()));
        }
        let content = String::from_utf8(bytes)
            .map_err(|_| ApiError::InvalidArgument("File is not valid UTF-8".into()))?;
        Ok(sworm_protocol::files::FileContent {
            content,
            version: final_version,
        })
    }
}

impl WorkspaceRouter {
    pub fn cancel_file_read(&self, owner: &str, request_id: &str) {
        self.inner.file_streams.cancel(owner, request_id);
    }

    pub async fn read_file_stream(
        &self,
        owner: &str,
        request_id: &str,
        project_path: String,
        file_path: String,
        version: String,
        size: u64,
        progress: impl Fn(u64, u64) + Send,
    ) -> Result<sworm_protocol::files::FileContent, ApiError> {
        self.inner
            .file_streams
            .read(
                &self.inner,
                owner,
                request_id,
                project_path,
                file_path,
                version,
                size,
                progress,
            )
            .await
    }
}
