use crate::RemoteError;
use serde::{de::DeserializeOwned, Serialize};
use std::io::{self, Write};
use sworm_protocol::rpc::MAX_FRAME_BYTES;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const JSON_TAG: u8 = 0;
pub const RAW_TAG: u8 = 1;

/// One decoded tagged frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame<T> {
    Json(T),
    Raw(Vec<u8>),
}

/// Encode a JSON tag and body, limiting the body without counting the tag.
pub fn encode_json<T: Serialize>(value: &T, limit: usize) -> Result<Vec<u8>, RemoteError> {
    let mut body = LimitedBuffer::new(limit);
    serde_json::to_writer(&mut body, value)
        .map_err(|error| RemoteError::transport("encode frame", error))?;
    Ok(body.bytes)
}

/// Write `u32 body length + JSON tag + JSON body`.
pub async fn write_frame<T: Serialize>(
    send: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), RemoteError> {
    write_frame_with_limit(send, value, MAX_FRAME_BYTES).await
}

/// Write one JSON frame with a caller-selected body limit.
pub async fn write_frame_with_limit<T: Serialize>(
    send: &mut (impl AsyncWrite + Unpin),
    value: &T,
    limit: usize,
) -> Result<(), RemoteError> {
    let bytes = encode_json(value, limit)?;
    let body_length = bytes.len() - 1;
    let length = u32::try_from(body_length).map_err(|_| {
        RemoteError::Transport(format!("frame of {body_length} bytes exceeds u32 length"))
    })?;
    send.write_all(&length.to_be_bytes())
        .await
        .map_err(|error| RemoteError::transport("write frame length", error))?;
    send.write_all(&bytes)
        .await
        .map_err(|error| RemoteError::transport("write JSON frame", error))?;
    Ok(())
}

/// Write `u32 body length + raw tag + bytes`.
pub async fn write_raw_frame(
    send: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
) -> Result<(), RemoteError> {
    write_raw_frame_parts(send, &[], bytes, MAX_FRAME_BYTES).await
}

/// Write one raw frame without copying its prefix and body into a new buffer.
pub async fn write_raw_frame_parts<W: AsyncWrite + Unpin>(
    send: &mut W,
    prefix: &[u8],
    bytes: &[u8],
    limit: usize,
) -> Result<(), RemoteError> {
    let body_length = prefix
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| RemoteError::Transport("stream frame size overflow".to_owned()))?;
    if body_length > limit {
        return Err(RemoteError::Transport(format!(
            "frame of {body_length} bytes exceeds {limit}-byte limit"
        )));
    }
    let length = u32::try_from(body_length).map_err(|_| {
        RemoteError::Transport(format!("frame of {body_length} bytes exceeds u32 length"))
    })?;
    let mut header = [0; 5];
    header[..4].copy_from_slice(&length.to_be_bytes());
    header[4] = RAW_TAG;
    send.write_all(&header)
        .await
        .map_err(|error| RemoteError::transport("write frame header", error))?;
    send.write_all(prefix)
        .await
        .map_err(|error| RemoteError::transport("write frame prefix", error))?;
    send.write_all(bytes)
        .await
        .map_err(|error| RemoteError::transport("write frame body", error))?;
    Ok(())
}

/// Read one tagged JSON or raw frame, bounded by the protocol ceiling.
pub async fn read_tagged_frame<T: DeserializeOwned>(
    recv: &mut (impl AsyncRead + Unpin),
) -> Result<Frame<T>, RemoteError> {
    read_tagged_frame_with_limit(recv, MAX_FRAME_BYTES).await
}

/// Read one tagged JSON or raw frame with a caller-selected body limit.
pub async fn read_tagged_frame_with_limit<T: DeserializeOwned>(
    recv: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> Result<Frame<T>, RemoteError> {
    let mut length = [0; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|error| RemoteError::transport("read frame length", error))?;
    let length = u32::from_be_bytes(length) as usize;
    if length > limit {
        return Err(RemoteError::Transport(format!(
            "frame of {length} bytes exceeds {limit}-byte limit"
        )));
    }

    let mut tag = [0];
    recv.read_exact(&mut tag)
        .await
        .map_err(|error| RemoteError::transport("read frame tag", error))?;
    if !matches!(tag[0], JSON_TAG | RAW_TAG) {
        return Err(RemoteError::Transport(format!(
            "unknown frame tag {}",
            tag[0]
        )));
    }

    let mut body = vec![0; length];
    recv.read_exact(&mut body)
        .await
        .map_err(|error| RemoteError::transport("read frame body", error))?;
    match tag[0] {
        JSON_TAG => serde_json::from_slice(&body)
            .map(Frame::Json)
            .map_err(|error| RemoteError::transport("decode frame", error)),
        RAW_TAG => Ok(Frame::Raw(body)),
        _ => unreachable!(),
    }
}

/// Read one JSON frame, rejecting raw frames.
pub async fn read_frame<T: DeserializeOwned>(
    recv: &mut (impl AsyncRead + Unpin),
) -> Result<T, RemoteError> {
    read_frame_with_limit(recv, MAX_FRAME_BYTES).await
}

/// Read one JSON frame with a caller-selected body limit, rejecting raw frames.
pub async fn read_frame_with_limit<T: DeserializeOwned>(
    recv: &mut (impl AsyncRead + Unpin),
    limit: usize,
) -> Result<T, RemoteError> {
    match read_tagged_frame_with_limit(recv, limit).await? {
        Frame::Json(value) => Ok(value),
        Frame::Raw(_) => Err(RemoteError::Transport(
            "expected JSON frame, received raw frame".to_owned(),
        )),
    }
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: vec![JSON_TAG],
            limit,
        }
    }
}

impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len() - 1) {
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

        write_frame(&mut send, &json!({"answer": 42}))
            .await
            .unwrap();
        write_raw_frame(&mut send, b"\0raw").await.unwrap();

        assert_eq!(
            read_tagged_frame::<serde_json::Value>(&mut recv)
                .await
                .unwrap(),
            Frame::Json(json!({"answer": 42}))
        );
        assert_eq!(
            read_tagged_frame::<serde_json::Value>(&mut recv)
                .await
                .unwrap(),
            Frame::Raw(b"\0raw".to_vec())
        );
    }

    #[tokio::test]
    async fn raw_layout_is_length_then_tag_then_body() {
        let (mut send, mut recv) = duplex(16);
        write_raw_frame_parts(&mut send, b"a", b"bc", 3)
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

        let error = read_tagged_frame_with_limit::<serde_json::Value>(&mut recv, 4)
            .await
            .unwrap_err();
        assert!(matches!(error, RemoteError::Transport(_)));
    }

    #[tokio::test]
    async fn unknown_tag_is_rejected() {
        let (mut send, mut recv) = duplex(16);
        send.write_all(&0u32.to_be_bytes()).await.unwrap();
        send.write_all(&[2]).await.unwrap();

        let error = read_tagged_frame_with_limit::<serde_json::Value>(&mut recv, 4)
            .await
            .unwrap_err();
        assert!(matches!(error, RemoteError::Transport(_)));
    }

    #[tokio::test]
    async fn json_reader_rejects_raw_frame() {
        let (mut send, mut recv) = duplex(16);
        write_raw_frame(&mut send, b"x").await.unwrap();

        let error = read_frame::<serde_json::Value>(&mut recv)
            .await
            .unwrap_err();
        assert!(matches!(error, RemoteError::Transport(_)));
    }

    #[tokio::test]
    async fn raw_writer_enforces_body_limit() {
        let mut send = Vec::new();

        let error = write_raw_frame_parts(&mut send, b"too ", b"long", 7)
            .await
            .unwrap_err();
        assert!(matches!(error, RemoteError::Transport(_)));
        assert!(send.is_empty(), "rejected frame wrote partial bytes");
    }

    #[tokio::test]
    async fn json_body_limit_excludes_tag_and_rejects_before_writing() {
        assert_eq!(encode_json(&42, 2).unwrap(), [JSON_TAG, b'4', b'2']);
        assert!(encode_json(&42, 1).is_err());

        let mut send = Vec::new();
        assert!(write_frame_with_limit(&mut send, &42, 1).await.is_err());
        assert!(send.is_empty(), "rejected frame wrote partial bytes");
        write_frame_with_limit(&mut send, &42, 2).await.unwrap();
        assert_eq!(send, [0, 0, 0, 2, JSON_TAG, b'4', b'2']);
        assert_eq!(
            read_frame_with_limit::<u32>(&mut send.as_slice(), 2)
                .await
                .unwrap(),
            42
        );
    }
}
