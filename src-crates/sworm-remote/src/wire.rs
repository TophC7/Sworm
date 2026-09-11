use crate::RemoteError;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Write};
use sworm_protocol::rpc::MAX_FRAME_BYTES;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const JSON_TAG: u8 = 0;
const RAW_TAG: u8 = 1;

/// One decoded tagged frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame<T> {
    Json(T),
    Raw(Vec<u8>),
}

/// Write `u32 body length + JSON tag + JSON body`.
pub async fn write_frame<T: Serialize>(
    send: &mut quinn::SendStream,
    value: &T,
) -> Result<(), RemoteError> {
    write_json_to(send, value, MAX_FRAME_BYTES).await
}

/// Write one JSON frame with a caller-selected body limit.
pub async fn write_frame_with_limit<T: Serialize>(
    send: &mut quinn::SendStream,
    value: &T,
    limit: usize,
) -> Result<(), RemoteError> {
    write_json_to(send, value, limit).await
}

/// Write `u32 body length + raw tag + bytes`.
pub async fn write_raw_frame(
    send: &mut quinn::SendStream,
    bytes: &[u8],
) -> Result<(), RemoteError> {
    write_tagged_body(send, RAW_TAG, bytes, MAX_FRAME_BYTES).await
}

/// Read one tagged JSON or raw frame, bounded by the protocol ceiling.
pub async fn read_tagged_frame<T: DeserializeOwned>(
    recv: &mut quinn::RecvStream,
) -> Result<Frame<T>, RemoteError> {
    read_tagged_from(recv, MAX_FRAME_BYTES).await
}

/// Read one tagged JSON or raw frame with a caller-selected body limit.
pub async fn read_tagged_frame_with_limit<T: DeserializeOwned>(
    recv: &mut quinn::RecvStream,
    limit: usize,
) -> Result<Frame<T>, RemoteError> {
    read_tagged_from(recv, limit).await
}

/// Read one JSON frame, rejecting raw frames.
pub async fn read_frame<T: DeserializeOwned>(
    recv: &mut quinn::RecvStream,
) -> Result<T, RemoteError> {
    read_json_from(recv, MAX_FRAME_BYTES).await
}

/// Read one JSON frame with a caller-selected body limit, rejecting raw frames.
pub async fn read_frame_with_limit<T: DeserializeOwned>(
    recv: &mut quinn::RecvStream,
    limit: usize,
) -> Result<T, RemoteError> {
    read_json_from(recv, limit).await
}

async fn write_json_to<T, W>(send: &mut W, value: &T, limit: usize) -> Result<(), RemoteError>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let mut body = LimitedBuffer::new(limit);
    serde_json::to_writer(&mut body, value)
        .map_err(|error| RemoteError::Transport(format!("encode frame: {error}")))?;
    write_tagged_body(send, JSON_TAG, &body.bytes, limit).await
}

async fn write_tagged_body<W>(
    send: &mut W,
    tag: u8,
    body: &[u8],
    limit: usize,
) -> Result<(), RemoteError>
where
    W: AsyncWrite + Unpin,
{
    if body.len() > limit {
        return Err(RemoteError::Transport(format!(
            "frame of {} bytes exceeds {limit}-byte limit",
            body.len()
        )));
    }
    let length = u32::try_from(body.len()).map_err(|_| {
        RemoteError::Transport(format!("frame of {} bytes exceeds u32 length", body.len()))
    })?;
    send.write_all(&length.to_be_bytes())
        .await
        .map_err(|error| RemoteError::Transport(format!("write frame length: {error}")))?;
    send.write_all(&[tag])
        .await
        .map_err(|error| RemoteError::Transport(format!("write frame tag: {error}")))?;
    send.write_all(body)
        .await
        .map_err(|error| RemoteError::Transport(format!("write frame body: {error}")))?;
    Ok(())
}

async fn read_json_from<T, R>(recv: &mut R, limit: usize) -> Result<T, RemoteError>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    match read_tagged_from(recv, limit).await? {
        Frame::Json(value) => Ok(value),
        Frame::Raw(_) => Err(RemoteError::Transport(
            "expected JSON frame, received raw frame".to_owned(),
        )),
    }
}

async fn read_tagged_from<T, R>(recv: &mut R, limit: usize) -> Result<Frame<T>, RemoteError>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
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

    let mut tag = [0];
    recv.read_exact(&mut tag)
        .await
        .map_err(|error| RemoteError::Transport(format!("read frame tag: {error}")))?;
    if !matches!(tag[0], JSON_TAG | RAW_TAG) {
        return Err(RemoteError::Transport(format!(
            "unknown frame tag {}",
            tag[0]
        )));
    }

    let mut body = vec![0; length];
    recv.read_exact(&mut body)
        .await
        .map_err(|error| RemoteError::Transport(format!("read frame body: {error}")))?;
    match tag[0] {
        JSON_TAG => serde_json::from_slice(&body)
            .map(Frame::Json)
            .map_err(|error| RemoteError::Transport(format!("decode frame: {error}"))),
        RAW_TAG => Ok(Frame::Raw(body)),
        _ => unreachable!(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::duplex;

    #[tokio::test]
    async fn tagged_json_and_raw_frames_round_trip() {
        let (mut send, mut recv) = duplex(128);

        write_json_to(&mut send, &json!({"answer": 42}), 64)
            .await
            .unwrap();
        write_tagged_body(&mut send, RAW_TAG, b"\0raw", 64)
            .await
            .unwrap();

        assert_eq!(
            read_tagged_from::<serde_json::Value, _>(&mut recv, 64)
                .await
                .unwrap(),
            Frame::Json(json!({"answer": 42}))
        );
        assert_eq!(
            read_tagged_from::<serde_json::Value, _>(&mut recv, 64)
                .await
                .unwrap(),
            Frame::Raw(b"\0raw".to_vec())
        );
    }

    #[tokio::test]
    async fn raw_layout_is_length_then_tag_then_body() {
        let (mut send, mut recv) = duplex(16);
        write_tagged_body(&mut send, RAW_TAG, b"abc", 3)
            .await
            .unwrap();

        let mut encoded = [0; 8];
        recv.read_exact(&mut encoded).await.unwrap();
        assert_eq!(encoded, [0, 0, 0, 3, RAW_TAG, b'a', b'b', b'c']);
    }

    #[tokio::test]
    async fn length_is_checked_before_allocating_or_reading_body() {
        let (mut send, mut recv) = duplex(16);
        send.write_all(&5u32.to_be_bytes()).await.unwrap();

        let error = read_tagged_from::<serde_json::Value, _>(&mut recv, 4)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "frame of 5 bytes exceeds 4-byte limit");
    }

    #[tokio::test]
    async fn unknown_tag_is_rejected() {
        let (mut send, mut recv) = duplex(16);
        send.write_all(&0u32.to_be_bytes()).await.unwrap();
        send.write_all(&[2]).await.unwrap();

        let error = read_tagged_from::<serde_json::Value, _>(&mut recv, 4)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "unknown frame tag 2");
    }

    #[tokio::test]
    async fn json_reader_rejects_raw_frame() {
        let (mut send, mut recv) = duplex(16);
        write_tagged_body(&mut send, RAW_TAG, b"x", 4)
            .await
            .unwrap();

        let error = read_json_from::<serde_json::Value, _>(&mut recv, 4)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "expected JSON frame, received raw frame");
    }

    #[tokio::test]
    async fn raw_writer_enforces_body_limit() {
        let (mut send, _) = duplex(16);

        let error = write_tagged_body(&mut send, RAW_TAG, b"too long", 3)
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "frame of 8 bytes exceeds 3-byte limit");
    }
}
