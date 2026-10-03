use crate::{
    config, dispatch, events, lsp_stream, pty_stream,
    stream::{StreamReader, StreamWriter},
    web,
};
use anyhow::Context;
use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use sworm_core::{services::completed_runs::CompletedRunStore, Host};
use sworm_protocol::rpc::{Open, Response, WireError, MAX_FRAME_BYTES, MAX_REQUEST_FRAME_BYTES};
use sworm_remote::{
    tls::{peer_fingerprint, server_config},
    wire::{read_frame_with_limit, write_frame},
    Fingerprint,
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
    /// Defaults to `<config_dir>/server.jsonc`.
    pub config_file: Option<PathBuf>,
    pub listen: Option<SocketAddr>,
    /// Frontend used when the config sets no `web.assets_dir`.
    pub web_assets_dir: Option<PathBuf>,
}

pub struct ServerHandle {
    pub local_addr: SocketAddr,
    pub web_addr: Option<SocketAddr>,
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
    let config_path = options
        .config_file
        .unwrap_or_else(|| config::default_path(&options.config_dir));
    let loaded = config::load(&config_path)?;
    let web = if let Some(web_config) = &loaded.web {
        let assets = config::resolve_web_assets(
            web_config,
            &config_path,
            options.web_assets_dir.as_deref(),
        )?;
        let allowed_origins = web_config
            .allowed_origins
            .iter()
            .map(|origin| {
                web::parse_origin(origin).with_context(|| {
                    format!("web.allowed_origins entry {origin:?} is not http(s)://host[:port]")
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let listener = tokio::net::TcpListener::bind(web_config.listen)
            .await
            .with_context(|| format!("bind web server to {}", web_config.listen))?;
        Some((listener, assets, allowed_origins))
    } else {
        None
    };
    let listen = options.listen.unwrap_or(loaded.listen);
    let identity = config::identity(&options.config_dir, &loaded)?;
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
    host.configure_completed_runs(Arc::clone(&completed));

    let mut quic_config = server_config(&identity)?;
    quic_config
        .max_incoming(MAX_CONNECTIONS)
        .incoming_buffer_size(1024 * 1024)
        .incoming_buffer_size_total(16 * 1024 * 1024);

    let endpoint = quinn::Endpoint::server(quic_config, listen)
        .with_context(|| format!("bind QUIC server to {listen}"))?;
    let local_addr = endpoint.local_addr()?;
    let context = Arc::new(dispatch::ServerContext::new(
        options.config_dir,
        loaded.authorized_keys_file,
        host_events,
    ));
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let request_permits = Arc::new(Semaphore::new(MAX_ACTIVE_REQUESTS));
    let web_addr = web
        .as_ref()
        .map(|(listener, _, _)| listener.local_addr())
        .transpose()?;
    let (shutdown, shutdown_rx) = watch::channel(false);
    let task = tokio::spawn(supervise(
        endpoint,
        web,
        host,
        context,
        permits,
        request_permits,
        shutdown.clone(),
        shutdown_rx,
    ));

    Ok(ServerHandle {
        local_addr,
        web_addr,
        fingerprint,
        shutdown,
        task,
    })
}

async fn supervise(
    endpoint: quinn::Endpoint,
    web_listener: Option<(tokio::net::TcpListener, PathBuf, Vec<web::Origin>)>,
    host: Arc<Host>,
    context: Arc<dispatch::ServerContext>,
    permits: Arc<Semaphore>,
    request_permits: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    shutdown: watch::Receiver<bool>,
) {
    let mut quic = tokio::spawn(run_server(
        endpoint,
        Arc::clone(&host),
        Arc::clone(&context),
        Arc::clone(&permits),
        Arc::clone(&request_permits),
        stop.clone(),
        shutdown.clone(),
    ));
    if let Some((listener, assets, allowed_origins)) = web_listener {
        let mut web = tokio::spawn(web::run(
            listener,
            assets,
            web::WebState::new(
                Arc::clone(&host),
                Arc::clone(&context),
                permits,
                request_permits,
                allowed_origins,
            ),
            stop.clone(),
            shutdown,
        ));
        tokio::select! {
            result = &mut quic => {
                if let Err(error) = result { tracing::error!(%error, "QUIC listener failed"); }
                let _ = stop.send(true);
                if let Err(error) = web.await { tracing::error!(%error, "web listener failed"); }
            }
            result = &mut web => {
                if let Err(error) = result { tracing::error!(%error, "web listener failed"); }
                let _ = stop.send(true);
                if let Err(error) = quic.await { tracing::error!(%error, "QUIC listener failed"); }
            }
        }
    } else if let Err(error) = quic.await {
        tracing::error!(%error, "QUIC listener failed");
    }
    // Registry transitions started by any socket or RPC finish before the Host stops.
    context.workbenches.shutdown().await;
    if let Err(error) = tokio::task::spawn_blocking(move || host.shutdown()).await {
        tracing::error!(%error, "host shutdown task failed");
    }
}

async fn run_server(
    endpoint: quinn::Endpoint,
    host: Arc<Host>,
    context: Arc<dispatch::ServerContext>,
    permits: Arc<Semaphore>,
    request_permits: Arc<Semaphore>,
    stop: watch::Sender<bool>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut connections = JoinSet::new();

    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    let _ = stop.send(true);
                    break;
                };
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
    // Host shutdown belongs to the combined supervisor, after both listeners drain.
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
    let static_keys = context.authorized_keys_file.clone();
    let authorized = match tokio::task::spawn_blocking(move || {
        crate::auth::is_authorized(&config_dir, fingerprint, static_keys.as_deref())
    })
    .await
    {
        Ok(authorized) => authorized,
        Err(error) => {
            tracing::warn!(%error, "authorized key lookup task failed");
            false
        }
    };
    if !authorized {
        tracing::info!(%fingerprint, "unpaired client connected; pair it or add this fingerprint to authorized keys");
    }
    let session = Arc::new(Mutex::new(dispatch::Session::new(
        Some(fingerprint),
        authorized,
        dispatch::SessionScope::Quic,
    )));
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
    let (leases, watchers) = {
        let session = session.lock().await;
        (session.leases.clone(), session.lease_watchers.clone())
    };
    let controls: Vec<_> = leases
        .lock()
        .await
        .values()
        .map(|lease| Arc::clone(&lease.control))
        .collect();
    for control in controls {
        control.retire(crate::workbenches::Terminal::Disconnected);
    }
    watchers.close();
    watchers.wait().await;

    if let Err(error) = cleanup_session(&host, &context, &session).await {
        tracing::error!(?error, "connection cleanup failed");
    }
}

/// Attempts every release and reports the first failure. Released folders
/// leave the Session first, so a failure is never retried blindly: a
/// JoinError may follow a partially applied reference-count decrement.
pub(crate) async fn cleanup_session(
    host: &Arc<Host>,
    context: &Arc<dispatch::ServerContext>,
    session: &Arc<Mutex<dispatch::Session>>,
) -> Result<(), WireError> {
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
    let mut result = Ok(());
    for folder in folders {
        if let Err(error) = context
            .release_folder(host, folder, subscriber_id.clone())
            .await
        {
            result = result.and(Err(error));
        }
    }
    let host = Arc::clone(host);
    if let Err(error) = tokio::task::spawn_blocking(move || {
        host.file_watchers.release_subscriber(&subscriber_id);
    })
    .await
    {
        result = result.and(Err(WireError::Internal {
            message: format!("connection resource release task failed: {error}"),
        }));
    }
    result
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
    // A paired client may open an RPC with a file body (writes, formatting
    // text), so the budget is chosen from this connection's auth state before
    // any body is read: a stranger still cannot make the daemon allocate more
    // than the intake cap.
    let limit = if session.lock().await.authorized {
        MAX_FRAME_BYTES
    } else {
        MAX_REQUEST_FRAME_BYTES
    };
    let open = match timeout(
        STREAM_READ_TIMEOUT,
        read_frame_with_limit::<Open>(&mut recv, limit),
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
    if !matches!(open, Open::Rpc(_) | Open::FileRead { .. }) && !session.lock().await.authorized {
        let response: Response = Err(dispatch::unauthorized(
            "client is not paired with this server",
        ));
        if write_response(&mut send, &response).await {
            close_unauthorized(&connection, &mut send).await;
        }
        return;
    }

    let response = match open {
        Open::FileRead {
            project_path,
            file_path,
            version,
        } => {
            let response =
                dispatch::claim_file_read(&host, &context, &session, &project_path).await;
            if let Err(error) = response {
                let unauthorized = matches!(error, WireError::Unauthorized { .. });
                let frame = sworm_protocol::rpc::FileReadDown::Error { error };
                let _ = timeout(STREAM_WRITE_TIMEOUT, write_frame(&mut send, &frame)).await;
                let _ = send.finish();
                if unauthorized {
                    close_unauthorized(&connection, &mut send).await;
                }
                return;
            }
            crate::file_stream::run(
                host,
                project_path,
                file_path,
                version,
                StreamWriter::quic(send, connection),
                StreamReader::quic(recv),
                shutdown,
            )
            .await;
            return;
        }
        Open::Rpc(request) => {
            // Bound RPC execution only. Stream intake and response backpressure hold no permit.
            let Ok(_permit) = request_permits.acquire_owned().await else {
                return;
            };
            dispatch::handle(&host, &context, &session, request).await
        }
        Open::WorkbenchRpc { workbench, request } => {
            if matches!(request, sworm_protocol::rpc::Request::WorkbenchClose { .. }) {
                // Close cannot wait for its own admitted work to drain.
                dispatch::handle(&host, &context, &session, request).await
            } else {
                let leases = session.lock().await.leases.clone();
                let admitted = leases.lock().await.get(&workbench).and_then(|lease| {
                    lease
                        .control
                        .track()
                        .map(|tracking| (lease.owner.clone(), tracking))
                });
                if let Some((owner, _tracking)) = admitted {
                    let Ok(_permit) = request_permits.acquire_owned().await else {
                        return;
                    };
                    dispatch::handle_scoped(
                        &host,
                        &context,
                        &session,
                        dispatch::SessionScope::Workbench {
                            id: workbench.clone(),
                            owner,
                        },
                        request,
                    )
                    .await
                } else {
                    Err(WireError::NotController { workbench })
                }
            }
        }
        Open::Events => {
            events::run(
                session,
                context.host_events.clone(),
                connection,
                send,
                shutdown,
            )
            .await;
            return;
        }
        Open::Pty { run_id, cursor } => {
            pty_stream::run(
                host,
                context,
                run_id,
                cursor,
                StreamWriter::quic(send, connection),
                StreamReader::quic(recv),
                shutdown,
            )
            .await;
            return;
        }
        Open::Lsp { session_id } => {
            // The lease an LSP stream claims is bound to this connection, so
            // the session's owner travels with the stream.
            let owner = session.lock().await.subscriber_id.clone();
            lsp_stream::run(
                host,
                context,
                session_id,
                owner,
                StreamWriter::quic(send, connection),
                StreamReader::quic(recv),
                shutdown,
            )
            .await;
            return;
        }
    };
    let unauthorized = matches!(&response, Err(WireError::Unauthorized { .. }));
    if !write_response(&mut send, &response).await {
        return;
    }
    if unauthorized {
        close_unauthorized(&connection, &mut send).await;
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
