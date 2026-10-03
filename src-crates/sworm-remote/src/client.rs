use crate::{
    identity::{Fingerprint, Identity},
    tls::{client_config, peer_fingerprint},
    wire::{read_frame, write_frame_with_limit},
    RemoteError,
};
use serde::Serialize;
use std::{
    collections::VecDeque,
    future::Future,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use sworm_protocol::rpc::{
    Open, OpenRpc, OpenWorkbenchRpc, Reply, Request, Response, WireError, MAX_FRAME_BYTES,
    MAX_REQUEST_FRAME_BYTES,
};
use tokio::{
    task::JoinSet,
    time::{sleep, timeout},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const ADDRESS_STAGGER: Duration = Duration::from_millis(250);

/// One authenticated QUIC connection to a sworm server.
pub struct RemoteClient {
    _endpoint: quinn::Endpoint,
    connection: quinn::Connection,
}

impl RemoteClient {
    /// Resolve a host and race pinned connections across staggered address families.
    pub async fn connect_host(
        endpoint: &quinn::Endpoint,
        address: &str,
        identity: Arc<Identity>,
        expected: Fingerprint,
    ) -> Result<Self, RemoteError> {
        let addresses = tokio::net::lookup_host(address)
            .await
            .map_err(|error| RemoteError::transport(format!("{address}: cannot resolve"), error))?
            .collect::<Vec<_>>();
        if addresses.is_empty() {
            return Err(RemoteError::transport(
                format!("{address}: cannot resolve"),
                "no resolved addresses",
            ));
        }

        let mut attempts = JoinSet::new();
        for (index, address) in interleave_addresses(addresses).into_iter().enumerate() {
            let endpoint = endpoint.clone();
            let identity = Arc::clone(&identity);
            attempts.spawn(async move {
                if index > 0 {
                    sleep(ADDRESS_STAGGER * index as u32).await;
                }
                Self::connect(&endpoint, address, &identity, expected).await
            });
        }

        let mut last_error = None;
        while let Some(result) = attempts.join_next().await {
            match result {
                Ok(Ok(client)) => {
                    attempts.abort_all();
                    while let Some(loser) = attempts.join_next().await {
                        if let Ok(Ok(loser)) = loser {
                            loser.close();
                        }
                    }
                    return Ok(client);
                }
                Ok(Err(error)) => last_error = Some(error),
                Err(error) if !error.is_cancelled() => {
                    last_error = Some(RemoteError::transport("connection attempt failed", error));
                }
                Err(_) => {}
            }
        }
        Err(last_error
            .unwrap_or_else(|| RemoteError::Transport("connection attempts cancelled".to_owned())))
    }

    /// Connect only when the server certificate matches the out-of-band fingerprint.
    pub async fn connect(
        endpoint: &quinn::Endpoint,
        addr: SocketAddr,
        identity: &Identity,
        expected: Fingerprint,
    ) -> Result<Self, RemoteError> {
        let connecting = endpoint
            .connect_with(client_config(identity, expected)?, addr, "sworm")
            .map_err(|error| RemoteError::transport("start QUIC connection", error))?;
        let connection = timeout(CONNECT_TIMEOUT, connecting)
            .await
            .map_err(|_| {
                RemoteError::Transport(format!(
                    "connection timed out after {} seconds",
                    CONNECT_TIMEOUT.as_secs()
                ))
            })?
            .map_err(|error| RemoteError::transport("connect to remote server", error))?;
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

    /// Run one request/response exchange. Normal RPCs have a deadline; attach
    /// recovery waits for its supervised operation or connection shutdown.
    ///
    /// An RPC may carry a file body, so its open frame is bounded by the
    /// whole-frame ceiling rather than the small intake cap. The daemon only
    /// accepts a frame that large once this connection is paired.
    pub async fn call(&self, request: &Request) -> Result<Reply, RemoteError> {
        let open = OpenRpc::new(request);
        if matches!(request, Request::WorkbenchRecover { .. }) {
            return self.exchange(&open).await;
        }
        self.call_open(&open).await
    }

    pub async fn call_as<T>(
        &self,
        request: &Request,
        pick: fn(Reply) -> Result<T, WireError>,
    ) -> Result<T, RemoteError> {
        pick(self.call(request).await?).map_err(RemoteError::Wire)
    }

    pub async fn call_workbench(
        &self,
        workbench: &str,
        request: &Request,
    ) -> Result<Reply, RemoteError> {
        self.call_open(&OpenWorkbenchRpc::new(workbench, request))
            .await
    }

    async fn call_open<T: Serialize>(&self, open: &T) -> Result<Reply, RemoteError> {
        timeout(REQUEST_TIMEOUT, self.exchange(open))
            .await
            .map_err(|_| {
                RemoteError::Timeout(format!(
                    "remote request timed out after {} seconds",
                    REQUEST_TIMEOUT.as_secs()
                ))
            })?
    }

    async fn exchange<T: Serialize>(&self, open: &T) -> Result<Reply, RemoteError> {
        let (mut send, mut recv) = self.open_frame(open, MAX_FRAME_BYTES).await?;
        send.finish()
            .map_err(|error| RemoteError::transport("finish request stream", error))?;
        match read_frame::<Response>(&mut recv).await? {
            Ok(reply) => Ok(reply),
            Err(error) => Err(RemoteError::Wire(error)),
        }
    }

    /// Open a lifetime stream. Callers own its timeout and shutdown policy.
    ///
    /// Every non-RPC open is small, so these stay inside the intake cap and
    /// keep working before pairing.
    pub async fn open_stream(
        &self,
        open: Open,
    ) -> Result<(quinn::SendStream, quinn::RecvStream), RemoteError> {
        self.open_frame(&open, MAX_REQUEST_FRAME_BYTES).await
    }

    async fn open_frame<T: Serialize>(
        &self,
        open: &T,
        limit: usize,
    ) -> Result<(quinn::SendStream, quinn::RecvStream), RemoteError> {
        let (mut send, recv) = self
            .connection
            .open_bi()
            .await
            .map_err(|error| RemoteError::Connection(format!("open stream: {error}")))?;
        write_frame_with_limit(&mut send, open, limit).await?;
        Ok((send, recv))
    }

    pub async fn pair(&self, token: &str, name: &str) -> Result<(), RemoteError> {
        self.call_as(
            &Request::Pair {
                token: token.to_owned(),
                name: name.to_owned(),
            },
            Reply::pair,
        )
        .await
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

pub fn client_endpoint() -> quinn::Endpoint {
    quinn::Endpoint::client("[::]:0".parse().expect("valid IPv6 wildcard"))
        .or_else(|_| quinn::Endpoint::client("0.0.0.0:0".parse().expect("valid IPv4 wildcard")))
        .expect("bind QUIC client endpoint")
}

fn interleave_addresses(addresses: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(first) = addresses.first() else {
        return addresses;
    };
    let first_is_v6 = matches!(first.ip(), IpAddr::V6(_));
    let (first_family, second_family): (VecDeque<_>, VecDeque<_>) = addresses
        .into_iter()
        .partition(|address| matches!(address.ip(), IpAddr::V6(_)) == first_is_v6);
    let mut families = [first_family, second_family];
    let mut ordered = Vec::with_capacity(families.iter().map(VecDeque::len).sum());
    while !families[0].is_empty() || !families[1].is_empty() {
        for family in &mut families {
            if let Some(address) = family.pop_front() {
                ordered.push(address);
            }
        }
    }
    ordered
}
