use axum::{
    body::Bytes,
    extract::ws::{CloseFrame, Message, WebSocket},
};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    io::{self, Write},
    time::Duration,
};
use sworm_protocol::rpc::MAX_FRAME_BYTES;
use sworm_remote::wire;
use tokio::{sync::watch, time::timeout};

const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

type WebSink = SplitSink<WebSocket, Message>;
type WebStream = SplitStream<WebSocket>;

pub(crate) enum Frame<T> {
    Json(T),
    Raw(Bytes),
}

/// Bound serialized JSON as it is produced, matching the QUIC frame ceiling.
struct TaggedJson(Vec<u8>);

impl Write for TaggedJson {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES.saturating_sub(self.0.len().saturating_sub(1)) {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "frame exceeds size limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A framed output stream. The web variant writes directly into the socket sink:
/// accepting a frame is never just enqueueing it in an unbounded writer actor.
pub(crate) enum StreamWriter {
    Quic {
        send: quinn::SendStream,
        connection: Option<quinn::Connection>,
    },
    Web {
        sink: WebSink,
        stop: watch::Receiver<Option<u16>>,
        policy_closed: bool,
    },
}

/// One input stream; dropping its web half signals the associated writer.
pub(crate) enum StreamReader {
    Quic(quinn::RecvStream),
    Web {
        stream: WebStream,
        stop: watch::Sender<Option<u16>>,
    },
}

impl StreamWriter {
    pub(crate) fn quic(send: quinn::SendStream, connection: Option<quinn::Connection>) -> Self {
        Self::Quic { send, connection }
    }

    pub(crate) fn web(sink: WebSink, stop: watch::Receiver<Option<u16>>) -> Self {
        Self::Web {
            sink,
            stop,
            policy_closed: false,
        }
    }

    pub(crate) async fn write_json<T: Serialize>(&mut self, value: &T) -> Result<(), String> {
        match self {
            Self::Quic { send, .. } => timeout(WRITE_TIMEOUT, wire::write_frame(send, value))
                .await
                .map_err(|_| "stream write timed out".to_owned())?
                .map_err(|error| error.to_string()),
            Self::Web { sink, .. } => {
                // Serialize straight into the final tagged message, including the tag.
                let mut bytes = TaggedJson(vec![0]);
                serde_json::to_writer(&mut bytes, value)
                    .map_err(|error| format!("encode frame: {error}"))?;
                timeout(WRITE_TIMEOUT, sink.send(Message::Binary(bytes.0.into())))
                    .await
                    .map_err(|_| "stream write timed out".to_owned())?
                    .map_err(|error| format!("write stream frame: {error}"))
            }
        }
    }

    pub(crate) async fn write_raw(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.write_raw_parts(&[], bytes).await
    }

    /// Keep PTY's offset and output in one final frame, without constructing
    /// another tagged body just to copy it into a WebSocket message.
    pub(crate) async fn write_raw_parts(
        &mut self,
        prefix: &[u8],
        bytes: &[u8],
    ) -> Result<(), String> {
        let length = prefix
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| "stream frame size overflow".to_owned())?;
        if length > MAX_FRAME_BYTES {
            return Err(format!("frame exceeds {MAX_FRAME_BYTES}-byte limit"));
        }
        match self {
            Self::Quic { send, .. } => {
                if prefix.is_empty() {
                    return timeout(WRITE_TIMEOUT, wire::write_raw_frame(send, bytes))
                        .await
                        .map_err(|_| "stream write timed out".to_owned())?
                        .map_err(|error| error.to_string());
                }
                let mut body = Vec::with_capacity(length);
                body.extend_from_slice(prefix);
                body.extend_from_slice(bytes);
                timeout(WRITE_TIMEOUT, wire::write_raw_frame(send, &body))
                    .await
                    .map_err(|_| "stream write timed out".to_owned())?
                    .map_err(|error| error.to_string())
            }
            Self::Web { sink, .. } => {
                let mut frame = Vec::with_capacity(length + 1);
                frame.push(1);
                frame.extend_from_slice(prefix);
                frame.extend_from_slice(bytes);
                timeout(WRITE_TIMEOUT, sink.send(Message::Binary(frame.into())))
                    .await
                    .map_err(|_| "stream write timed out".to_owned())?
                    .map_err(|error| format!("write stream frame: {error}"))
            }
        }
    }

    pub(crate) async fn finish(&mut self) -> Result<(), String> {
        match self {
            Self::Quic { send, .. } => send.finish().map_err(|error| error.to_string()),
            Self::Web {
                sink,
                stop,
                policy_closed,
            } => {
                let code = *stop.borrow();
                if !*policy_closed && matches!(code, Some(1008 | 1009)) {
                    send_policy_close(sink, code.unwrap()).await?;
                    *policy_closed = true;
                }
                timeout(WRITE_TIMEOUT, sink.close())
                    .await
                    .map_err(|_| "stream close timed out".to_owned())?
                    .map_err(|error| format!("close stream: {error}"))
            }
        }
    }

    pub(crate) async fn stopped(&mut self) {
        match self {
            Self::Quic { send, connection } => {
                if let Some(connection) = connection {
                    tokio::select! { _ = send.stopped() => {}, _ = connection.closed() => {} }
                } else {
                    let _ = send.stopped().await;
                }
            }
            Self::Web {
                sink,
                stop,
                policy_closed,
            } => {
                while stop.borrow().is_none() {
                    if stop.changed().await.is_err() {
                        return;
                    }
                }
                let code = *stop.borrow();
                if !*policy_closed && matches!(code, Some(1008 | 1009)) {
                    *policy_closed = true;
                    let _ = send_policy_close(sink, code.unwrap()).await;
                }
            }
        }
    }
}

async fn send_policy_close(sink: &mut WebSink, code: u16) -> Result<(), String> {
    timeout(
        WRITE_TIMEOUT,
        sink.send(Message::Close(Some(CloseFrame {
            code,
            reason: "".into(),
        }))),
    )
    .await
    .map_err(|_| "stream policy close timed out".to_owned())?
    .map_err(|error| format!("close stream: {error}"))
}

fn signal_stop(stop: &watch::Sender<Option<u16>>, code: u16) {
    if stop.borrow().is_none() {
        let _ = stop.send(Some(code));
    }
}

impl StreamReader {
    pub(crate) fn quic(recv: quinn::RecvStream) -> Self {
        Self::Quic(recv)
    }

    pub(crate) fn web(stream: WebStream, stop: watch::Sender<Option<u16>>) -> Self {
        Self::Web { stream, stop }
    }

    pub(crate) fn is_web(&self) -> bool {
        matches!(self, Self::Web { .. })
    }

    /// FileRead accepts no client application frames. Watch the web half solely
    /// for disconnect/control frames; unexpected input ends its writer.
    pub(crate) async fn observe_close(&mut self) -> Result<(), String> {
        if let Self::Web { stream, stop } = self {
            loop {
                let (result, code) = match stream.next().await {
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    Some(Ok(Message::Close(_))) | None => (Ok(()), 1000),
                    Some(Ok(Message::Binary(_) | Message::Text(_))) => (
                        Err("unexpected FileRead application input".to_owned()),
                        1008,
                    ),
                    Some(Err(error)) => (Err(format!("read file stream: {error}")), 1000),
                };
                signal_stop(stop, code);
                return result;
            }
        }
        Ok(())
    }

    pub(crate) async fn read_tagged<T: DeserializeOwned>(&mut self) -> Result<Frame<T>, String> {
        match self {
            Self::Quic(recv) => wire::read_tagged_frame(recv)
                .await
                .map_err(|error| error.to_string())
                .map(|frame| match frame {
                    wire::Frame::Json(value) => Frame::Json(value),
                    wire::Frame::Raw(bytes) => Frame::Raw(bytes.into()),
                }),
            Self::Web { stream, stop } => loop {
                let message = match stream.next().await {
                    Some(Ok(message)) => message,
                    Some(Err(error)) => {
                        signal_stop(stop, 1000);
                        return Err(format!("read stream frame: {error}"));
                    }
                    None => {
                        signal_stop(stop, 1000);
                        return Err("stream closed".to_owned());
                    }
                };
                match message {
                    Message::Binary(bytes) => {
                        if bytes.len() > MAX_FRAME_BYTES + 1 {
                            signal_stop(stop, 1009);
                            return Err("stream frame exceeds size limit".to_owned());
                        }
                        let result = match bytes.split_first() {
                            Some((&0, body)) => serde_json::from_slice(body)
                                .map(Frame::Json)
                                .map_err(|error| format!("decode stream frame: {error}")),
                            Some((&1, _)) => Ok(Frame::Raw(bytes.slice(1..))),
                            _ => Err("unknown or missing stream frame tag".to_owned()),
                        };
                        if result.is_err() {
                            signal_stop(stop, 1008);
                        }
                        return result;
                    }
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => {
                        signal_stop(stop, 1000);
                        return Err("stream closed".to_owned());
                    }
                    Message::Text(_) => {
                        signal_stop(stop, 1008);
                        return Err("expected binary stream frame".to_owned());
                    }
                }
            },
        }
    }

    pub(crate) async fn read_json<T: DeserializeOwned>(&mut self) -> Result<T, String> {
        match self {
            Self::Quic(recv) => wire::read_frame(recv)
                .await
                .map_err(|error| error.to_string()),
            Self::Web { .. } => match self.read_tagged().await? {
                Frame::Json(value) => Ok(value),
                Frame::Raw(_) => {
                    if let Self::Web { stop, .. } = self {
                        signal_stop(stop, 1008);
                    }
                    Err("expected JSON frame, received raw frame".to_owned())
                }
            },
        }
    }
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        if let Self::Web { stop, .. } = self {
            signal_stop(stop, 1000);
        }
    }
}
