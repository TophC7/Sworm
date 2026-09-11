use crate::{
    identity::{Fingerprint, Identity},
    tls::{client_config, peer_fingerprint},
    wire::{read_frame, write_frame_with_limit},
};
use std::{future::Future, net::SocketAddr, time::Duration};
use sworm_protocol::rpc::{Open, Reply, Request, Response, WireError, MAX_REQUEST_FRAME_BYTES};
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("{0}")]
    Transport(String),
    #[error("{0}")]
    Connection(String),
    #[error("{0:?}")]
    Wire(WireError),
    #[error("{0}")]
    Identity(String),
}

/// One authenticated QUIC connection to a sworm server.
pub struct RemoteClient {
    _endpoint: quinn::Endpoint,
    connection: quinn::Connection,
}

impl RemoteClient {
    /// Connect only when the server certificate matches the out-of-band fingerprint.
    pub async fn connect(
        endpoint: &quinn::Endpoint,
        addr: SocketAddr,
        identity: &Identity,
        expected: Fingerprint,
    ) -> Result<Self, RemoteError> {
        let connecting = endpoint
            .connect_with(client_config(identity, expected)?, addr, "sworm")
            .map_err(|error| transport_error("start QUIC connection", error))?;
        let connection = timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| {
                RemoteError::Transport(format!(
                    "connection timed out after {} seconds",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|error| transport_error("connect to remote server", error))?;
        if peer_fingerprint(&connection).is_none() {
            connection.close(0u32.into(), b"missing server certificate");
            return Err(RemoteError::Transport(
                "server did not present a certificate".to_string(),
            ));
        }
        Ok(Self {
            _endpoint: endpoint.clone(),
            connection,
        })
    }

    /// Run one bounded request/response exchange.
    pub async fn call(&self, request: &Request) -> Result<Reply, RemoteError> {
        timeout(REQUEST_TIMEOUT, async {
            let (mut send, mut recv) = self.open_stream(Open::Rpc(request.clone())).await?;
            send.finish()
                .map_err(|error| transport_error("finish request stream", error))?;
            match read_frame::<Response>(&mut recv).await? {
                Ok(reply) => Ok(reply),
                Err(error) => Err(RemoteError::Wire(error)),
            }
        })
        .await
        .map_err(|_| {
            RemoteError::Transport(format!(
                "remote request timed out after {} seconds",
                REQUEST_TIMEOUT.as_secs()
            ))
        })?
    }

    /// Open a lifetime stream. Callers own its timeout and shutdown policy.
    pub async fn open_stream(
        &self,
        open: Open,
    ) -> Result<(quinn::SendStream, quinn::RecvStream), RemoteError> {
        let (mut send, recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|error| connection_error("open stream", error))?;
        write_frame_with_limit(&mut send, &open, MAX_REQUEST_FRAME_BYTES).await?;
        Ok((send, recv))
    }

    pub async fn pair(&self, token: &str, name: &str) -> Result<(), RemoteError> {
        self.call(&Request::Pair {
            token: token.to_owned(),
            name: name.to_owned(),
        })
        .await?
        .pair()
        .map_err(RemoteError::Wire)
    }

    #[doc(hidden)]
    pub fn connection(&self) -> &quinn::Connection {
        &self.connection
    }

    /// Resolve when QUIC closes locally or remotely.
    pub fn closed(&self) -> impl Future<Output = quinn::ConnectionError> + '_ {
        self.connection.closed()
    }

    pub fn is_closed(&self) -> bool {
        self.connection.close_reason().is_some()
    }

    pub fn close(&self) {
        self.connection.close(0u32.into(), b"bye");
    }
}

fn transport_error(context: &str, error: impl std::fmt::Display) -> RemoteError {
    RemoteError::Transport(format!("{context}: {error}"))
}

fn connection_error(context: &str, error: impl std::fmt::Display) -> RemoteError {
    RemoteError::Connection(format!("{context}: {error}"))
}
