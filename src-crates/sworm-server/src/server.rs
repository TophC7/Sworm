use crate::{config, dispatch, events, pty_stream};
use anyhow::Context;
use std::{collections::HashSet, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use sworm_core::{services::completed_runs::CompletedRunStore, Host};
use sworm_protocol::rpc::{
    Open, Response, WireError, MAX_REQUEST_FRAME_BYTES, MAX_STREAMS_PER_CONNECTION,
};
use sworm_remote::{
    tls::{peer_fingerprint, server_config},
    wire::{read_frame_with_limit, write_frame},
    Fingerprint, Identity,
};
use tokio::{
    sync::{broadcast, watch, Mutex, Semaphore},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_READ_TIMEOUT: Duration = Duration::from_secs(30);
const STREAM_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const UNAUTHORIZED_ACK_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONNECTIONS: usize = 128;
const MAX_ACTIVE_REQUESTS: usize = 64;
const HOST_EVENT_CAPACITY: usize = 1024;
/// Transcripts of finished runs are bounded by both age and count: a daemon
/// that ran hundreds of tasks today must not fill its disk either way.
const COMPLETED_RUNS_KEPT: usize = 200;
const COMPLETED_RUN_DAYS: i64 = 7;

pub struct ServeOptions {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub listen: Option<SocketAddr>,
}

pub struct ServerHandle {
    pub local_addr: SocketAddr,
    pub fingerprint: Fingerprint,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl ServerHandle {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        if let Err(error) = self.task.await {
            tracing::error!(%error, "server task failed during shutdown");
        }
    }
}

pub async fn serve(options: ServeOptions) -> anyhow::Result<ServerHandle> {
    let loaded = config::load(&options.config_dir)?;
    let listen = options.listen.unwrap_or(loaded.listen);
    let identity = Identity::load_or_generate(&options.config_dir, "server")?;
    let fingerprint = identity.fingerprint();
    std::fs::create_dir_all(&options.data_dir)
        .with_context(|| format!("create data directory {}", options.data_dir.display()))?;
    let (host_events, _) = broadcast::channel(HOST_EVENT_CAPACITY);
    let event_sender = host_events.clone();
    let host = Arc::new(Host::new(
        options.data_dir.join("server.db"),
        Arc::new(move |event| {
            if let Some(event) = events::to_wire(event) {
                let _ = event_sender.send(Arc::new(event));
            }
            Ok(())
        }),
    )?);
    let completed = Arc::new(CompletedRunStore::new(
        Arc::clone(&host.db),
        COMPLETED_RUNS_KEPT,
        chrono::Duration::days(COMPLETED_RUN_DAYS),
    ));
    let store = Arc::clone(&completed);
    host.pty.on_completed_run(Arc::new(move |run| {
        let run_id = run.run_id.clone();
        if let Err(error) = store.put(&run) {
            tracing::error!(%error, run_id, "failed to store completed run");
        }
    }));

    let mut quic_config = server_config(&identity)?;
    let mut transport = quinn::TransportConfig::default();
    transport
        .max_concurrent_bidi_streams(MAX_STREAMS_PER_CONNECTION.into())
        .max_concurrent_uni_streams(0u32.into())
        .max_idle_timeout(Some(
            Duration::from_secs(30)
                .try_into()
                .expect("30 second QUIC idle timeout is representable"),
        ))
        .keep_alive_interval(Some(Duration::from_secs(10)));
    quic_config.transport_config(Arc::new(transport));
    quic_config
        .max_incoming(MAX_CONNECTIONS)
        .incoming_buffer_size(1024 * 1024)
        .incoming_buffer_size_total(16 * 1024 * 1024);

    let endpoint = quinn::Endpoint::server(quic_config, listen)
        .with_context(|| format!("bind QUIC server to {listen}"))?;
    let local_addr = endpoint.local_addr()?;
    let context = Arc::new(dispatch::ServerContext::new(
        options.config_dir,
        loaded.auth_token,
        host_events,
        completed,
    ));
    let (shutdown, shutdown_rx) = watch::channel(false);
    let task = tokio::spawn(run_server(endpoint, host, context, shutdown_rx));

    Ok(ServerHandle {
        local_addr,
        fingerprint,
        shutdown,
        task,
    })
}

async fn run_server(
    endpoint: quinn::Endpoint,
    host: Arc<Host>,
    context: Arc<dispatch::ServerContext>,
    mut shutdown: watch::Receiver<bool>,
) {
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let request_permits = Arc::new(Semaphore::new(MAX_ACTIVE_REQUESTS));
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else { break };
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    incoming.refuse();
                    continue;
                };
                let host = Arc::clone(&host);
                let context = Arc::clone(&context);
                let mut shutdown = shutdown.clone();
                let request_permits = Arc::clone(&request_permits);
                connections.spawn(async move {
                    let _permit = permit;
                    let connecting = match incoming.accept() {
                        Ok(connecting) => connecting,
                        Err(error) => {
                            tracing::warn!(%error, "failed to accept QUIC connection");
                            return;
                        }
                    };
                    let handshake = tokio::select! {
                        _ = shutdown.changed() => return,
                        result = timeout(HANDSHAKE_TIMEOUT, connecting) => result,
                    };
                    match handshake {
                        Ok(Ok(connection)) => run_connection(connection, host, context, request_permits, shutdown).await,
                        Ok(Err(error)) => tracing::warn!(%error, "QUIC handshake failed"),
                        Err(_) => tracing::warn!("QUIC handshake timed out"),
                    }
                });
            }
            Some(result) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = result {
                    tracing::error!(%error, "connection task failed");
                }
            }
        }
    }

    endpoint.close(0u32.into(), b"shutdown");
    while let Some(result) = connections.join_next().await {
        if let Err(error) = result {
            tracing::error!(%error, "connection task failed during shutdown");
        }
    }
    endpoint.wait_idle().await;
    host.shutdown();
}

async fn run_connection(
    connection: quinn::Connection,
    host: Arc<Host>,
    context: Arc<dispatch::ServerContext>,
    request_permits: Arc<Semaphore>,
    mut shutdown: watch::Receiver<bool>,
) {
    let Some(fingerprint) = peer_fingerprint(&connection) else {
        connection.close(1u32.into(), b"no client certificate");
        return;
    };
    let config_dir = context.config_dir.clone();
    let authorized = match tokio::task::spawn_blocking(move || {
        crate::auth::is_authorized(&config_dir, fingerprint)
    })
    .await
    {
        Ok(authorized) => authorized,
        Err(error) => {
            tracing::warn!(%error, "authorized key lookup task failed");
            false
        }
    };
    let session = Arc::new(Mutex::new(dispatch::Session {
        fingerprint,
        authorized,
        subscriber_id: uuid::Uuid::new_v4().to_string(),
        folders: HashSet::new(),
        next_events: 0,
        events: None,
    }));
    let mut streams = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    connection.close(0u32.into(), b"shutdown");
                    break;
                }
            }
            stream = connection.accept_bi() => match stream {
                Ok((send, recv)) => {
                    let connection = connection.clone();
                    let host = Arc::clone(&host);
                    let context = Arc::clone(&context);
                    let session = Arc::clone(&session);
                    let request_permits = Arc::clone(&request_permits);
                    let stream_shutdown = shutdown.clone();
                    streams.spawn(process_stream(
                        connection,
                        host,
                        context,
                        session,
                        request_permits,
                        send,
                        recv,
                        stream_shutdown,
                    ));
                }
                Err(_) => break,
            },
            Some(result) = streams.join_next(), if !streams.is_empty() => {
                if let Err(error) = result {
                    tracing::error!(%error, "request stream task failed");
                }
            }
        }
    }

    while let Some(result) = streams.join_next().await {
        if let Err(error) = result {
            tracing::error!(%error, "request stream task failed during connection shutdown");
        }
    }

    let (folders, subscriber_id) = {
        let mut session = session.lock().await;
        if let Some((_, stop)) = session.events.take() {
            let _ = stop.send(true);
        }
        (
            std::mem::take(&mut session.folders),
            session.subscriber_id.clone(),
        )
    };
    for folder in folders {
        if context.release_folder(&folder) {
            host.release_folder(&folder);
        }
    }
    host.file_watchers.release_subscriber(&subscriber_id);
}

async fn process_stream(
    connection: quinn::Connection,
    host: Arc<Host>,
    context: Arc<dispatch::ServerContext>,
    session: Arc<Mutex<dispatch::Session>>,
    request_permits: Arc<Semaphore>,
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
    shutdown: watch::Receiver<bool>,
) {
    let open = match timeout(
        STREAM_READ_TIMEOUT,
        read_frame_with_limit::<Open>(&mut recv, MAX_REQUEST_FRAME_BYTES),
    )
    .await
    {
        Ok(Ok(open)) => open,
        Ok(Err(error)) => {
            tracing::warn!(%error, "invalid stream open frame");
            return;
        }
        Err(_) => {
            tracing::warn!("stream open frame timed out");
            return;
        }
    };

    match open {
        Open::Rpc(request) => {
            // Bound RPC execution only. Stream intake and response backpressure hold no permit.
            let response = {
                let Ok(_permit) = request_permits.acquire_owned().await else {
                    return;
                };
                dispatch::handle(&host, &context, &session, request).await
            };
            let unauthorized = matches!(&response, Err(WireError::Unauthorized { .. }));
            if !write_response(&mut send, &response).await {
                return;
            }
            if unauthorized {
                close_unauthorized(&connection, &mut send).await;
            }
        }
        Open::Events => {
            if !session.lock().await.authorized {
                let response: Response = Err(dispatch::unauthorized(
                    "client is not paired with this server",
                ));
                if write_response(&mut send, &response).await {
                    close_unauthorized(&connection, &mut send).await;
                }
                return;
            }
            events::run(
                session,
                context.host_events.clone(),
                connection,
                send,
                shutdown,
            )
            .await;
        }
        Open::Pty { run_id, cursor } => {
            if !session.lock().await.authorized {
                let response: Response = Err(dispatch::unauthorized(
                    "client is not paired with this server",
                ));
                if write_response(&mut send, &response).await {
                    close_unauthorized(&connection, &mut send).await;
                }
                return;
            }
            pty_stream::run(
                host, context, run_id, cursor, connection, send, recv, shutdown,
            )
            .await;
        }
    }
}

async fn write_response(send: &mut quinn::SendStream, response: &Response) -> bool {
    match timeout(STREAM_WRITE_TIMEOUT, write_frame(send, response)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(%error, "failed to write response frame");
            return false;
        }
        Err(_) => {
            tracing::warn!("response frame timed out");
            return false;
        }
    }
    if let Err(error) = send.finish() {
        tracing::warn!(%error, "failed to finish response stream");
        return false;
    }
    true
}

async fn close_unauthorized(connection: &quinn::Connection, send: &mut quinn::SendStream) {
    // Let the peer read the stream error before CONNECTION_CLOSE invalidates in-flight data.
    let _ = timeout(UNAUTHORIZED_ACK_TIMEOUT, send.stopped()).await;
    connection.close(1u32.into(), b"unauthorized");
}
