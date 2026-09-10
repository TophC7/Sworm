use crate::RemoteError;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Write};
use sworm_protocol::rpc::MAX_FRAME_BYTES;

/// Write a big-endian `u32` length followed by one JSON value.
pub async fn write_frame<T: Serialize>(
    send: &mut quinn::SendStream,
    value: &T,
) -> Result<(), RemoteError> {
    let mut body = LimitedBuffer::new(MAX_FRAME_BYTES);
    serde_json::to_writer(&mut body, value)
        .map_err(|error| RemoteError::Transport(format!("encode frame: {error}")))?;
    send.write_all(&(body.bytes.len() as u32).to_be_bytes())
        .await
        .map_err(|error| RemoteError::Transport(format!("write frame length: {error}")))?;
    send.write_all(&body.bytes)
        .await
        .map_err(|error| RemoteError::Transport(format!("write frame body: {error}")))?;
    Ok(())
}

/// Read one JSON value after a big-endian `u32` length, bounded by the protocol ceiling.
pub async fn read_frame<T: DeserializeOwned>(
    recv: &mut quinn::RecvStream,
) -> Result<T, RemoteError> {
    read_frame_with_limit(recv, MAX_FRAME_BYTES).await
}

pub async fn read_frame_with_limit<T: DeserializeOwned>(
    recv: &mut quinn::RecvStream,
    limit: usize,
) -> Result<T, RemoteError> {
    let mut length = [0; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|error| RemoteError::Transport(format!("read frame length: {error}")))?;
    let length = u32::from_be_bytes(length) as usize;
    if length > limit {
        return Err(RemoteError::Transport(format!(
            "frame of {length} bytes exceeds {limit}-byte limit"
        )));
    }
    let mut body = vec![0; length];
    recv.read_exact(&mut body)
        .await
        .map_err(|error| RemoteError::Transport(format!("read frame body: {error}")))?;
    serde_json::from_slice(&body)
        .map_err(|error| RemoteError::Transport(format!("decode frame: {error}")))
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!("frame exceeds {}-byte limit", self.limit),
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
