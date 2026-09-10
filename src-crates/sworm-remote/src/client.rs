use crate::{
    identity::{Fingerprint, Identity},
    tls::{client_config, peer_fingerprint},
    wire::{read_frame, write_frame},
};
use serde::de::DeserializeOwned;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use sworm_protocol::rpc::{Request, Response, WireError};
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
        addr: SocketAddr,
        identity: &Identity,
        expected: Fingerprint,
    ) -> Result<Self, RemoteError> {
        let bind_addr = match addr {
            SocketAddr::V4(_) => SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0),
            SocketAddr::V6(_) => SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0),
        };
        let endpoint = quinn::Endpoint::client(bind_addr)
            .map_err(|error| transport_error("bind QUIC client endpoint", error))?;
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
            _endpoint: endpoint,
            connection,
        })
    }

    pub async fn call<T: DeserializeOwned>(&self, request: &Request) -> Result<T, RemoteError> {
        timeout(REQUEST_TIMEOUT, async {
            let (mut send, mut recv) = self
                .connection
                .open_bi()
                .await
                .map_err(|error| connection_error("open request stream", error))?;
            write_frame(&mut send, request).await?;
            send.finish()
                .map_err(|error| transport_error("finish request stream", error))?;
            match read_frame::<Response>(&mut recv).await? {
                Ok(value) => serde_json::from_value(value).map_err(|error| {
                    RemoteError::Transport(format!("decode response value: {error}"))
                }),
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

    pub async fn pair(&self, token: &str, name: &str) -> Result<(), RemoteError> {
        self.call::<serde_json::Value>(&Request::Pair {
            token: token.to_owned(),
            name: name.to_owned(),
        })
        .await?;
        Ok(())
    }

    #[doc(hidden)]
    pub fn connection(&self) -> &quinn::Connection {
        &self.connection
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
