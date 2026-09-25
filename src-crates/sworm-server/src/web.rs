use crate::{
    dispatch, events, file_stream, lsp_stream, pty_stream,
    server::cleanup_session,
    stream::{StreamReader, StreamWriter},
};
use axum::{
    extract::{
        ws::{close_code, CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{header, uri::Authority, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    future::IntoFuture,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};
use sworm_core::Host;
use sworm_protocol::rpc::{
    FileReadDown, Open, Request, Response as RpcResponse, MAX_FRAME_BYTES, MAX_REQUEST_FRAME_BYTES,
    MAX_STREAMS_PER_CONNECTION,
};
use tokio::{
    net::TcpListener,
    sync::{broadcast, mpsc, watch, Mutex, OwnedSemaphorePermit, Semaphore},
    time::{interval_at, timeout, Instant, MissedTickBehavior},
};
use tokio_util::task::TaskTracker;
use tower_http::services::{ServeDir, ServeFile};

const IO_TIMEOUT: Duration = Duration::from_secs(30);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(crate) struct WebState {
    host: Arc<Host>,
    context: Arc<dispatch::ServerContext>,
    connections: Arc<Semaphore>,
    requests: Arc<Semaphore>,
    controls: Arc<StdMutex<Registry>>,
    upgrades: TaskTracker,
}

#[derive(Default)]
struct Registry {
    closing: bool,
    controls: HashMap<String, Arc<Control>>,
}

struct Control {
    session: Arc<Mutex<dispatch::Session>>,
    admitted: Arc<Semaphore>,
    tasks: TaskTracker,
    stop: watch::Sender<bool>,
    live_ids: Arc<StdMutex<HashSet<u64>>>,
    close_code: AtomicU16,
}

struct Outgoing {
    message: Message,
    id: Option<u64>,
    _admission: Option<OwnedSemaphorePermit>,
}

struct RequestEnvelope {
    id: u64,
    request: Request,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Incoming {
    id: Option<u64>,
    request: Option<Request>,
    pong: Option<u32>,
}

#[derive(Serialize)]
struct Ready<'a> {
    ready: ReadyId<'a>,
}

#[derive(Serialize)]
struct ReadyId<'a> {
    connection_id: &'a str,
}

#[derive(Serialize)]
struct ReplyEnvelope<'a> {
    id: u64,
    response: &'a RpcResponse,
}

#[derive(Serialize)]
struct EventEnvelope<'a> {
    event: &'a sworm_protocol::rpc::HostEventWire,
}

impl WebState {
    pub(crate) fn new(
        host: Arc<Host>,
        context: Arc<dispatch::ServerContext>,
        connections: Arc<Semaphore>,
        requests: Arc<Semaphore>,
    ) -> Self {
        Self {
            host,
            context,
            connections,
            requests,
            controls: Arc::new(StdMutex::new(Registry::default())),
            upgrades: TaskTracker::new(),
        }
    }

    fn stop_all(&self) {
        let mut registry = self.controls.lock().expect("web registry poisoned");
        registry.closing = true;
        for control in registry.controls.values() {
            let _ = control.stop.send(true);
        }
    }
}

pub(crate) async fn run(
    listener: TcpListener,
    assets: PathBuf,
    state: WebState,
    stop: watch::Sender<bool>,
    mut shutdown: watch::Receiver<bool>,
) {
    let routes = Router::new()
        .route("/ws", get(control_upgrade))
        .route("/ws/stream", get(stream_upgrade))
        .fallback_service(
            ServeDir::new(&assets).fallback(ServeFile::new(assets.join("index.html"))),
        )
        .with_state(state.clone());
    let server = axum::serve(listener, routes)
        .with_graceful_shutdown({
            let mut shutdown = shutdown.clone();
            async move {
                let _ = shutdown.changed().await;
            }
        })
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => {
            if let Err(error) = result { tracing::error!(%error, "web listener exited"); }
            let _ = stop.send(true);
        },
        _ = shutdown.changed() => {
            state.stop_all();
            if let Err(error) = server.await { tracing::error!(%error, "web listener shutdown failed"); }
        }
    }
    state.stop_all();
    state.upgrades.close();
    state.upgrades.wait().await;
}

fn authority(value: &str, scheme: &str) -> Option<(String, u16)> {
    let parsed = Authority::from_str(value).ok()?;
    let host = parsed.host();
    if value.contains('@') || host.is_empty() {
        return None;
    }
    let port = match value.strip_prefix(host)? {
        "" if scheme == "https" => 443,
        "" => 80,
        explicit => explicit.strip_prefix(':')?.parse::<u16>().ok()?,
    };
    Some((host.to_ascii_lowercase(), port))
}

fn same_origin(headers: &HeaderMap) -> bool {
    if headers.get_all(header::ORIGIN).iter().count() != 1
        || headers.get_all(header::HOST).iter().count() != 1
    {
        return false;
    }
    let Some(origin) = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some((scheme, raw_authority)) = origin.split_once("://") else {
        return false;
    };
    let scheme = scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https")
        || raw_authority
            .chars()
            .any(|ch| matches!(ch, '/' | '?' | '#' | '\\'))
    {
        return false;
    }
    let Some(host) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    match (authority(raw_authority, &scheme), authority(host, &scheme)) {
        (Some(origin), Some(host)) => origin == host,
        _ => false,
    }
}

fn upgrade_limits(ws: WebSocketUpgrade) -> WebSocketUpgrade {
    ws.max_message_size(MAX_FRAME_BYTES + 1)
        .max_frame_size(MAX_FRAME_BYTES + 1)
}

async fn control_upgrade(
    State(state): State<WebState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if state
        .controls
        .lock()
        .expect("web registry poisoned")
        .closing
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let Ok(permit) = Arc::clone(&state.connections).try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let token = state.upgrades.token();
    upgrade_limits(ws)
        .on_upgrade(move |socket| async move {
            let _tracking = token;
            let _admission = permit;
            control_socket(socket, state).await;
        })
        .into_response()
}

async fn stream_upgrade(
    State(state): State<WebState>,
    headers: HeaderMap,
    Query(query): Query<HashMap<String, String>>,
    ws: WebSocketUpgrade,
) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(id) = query.get("connection_id") else {
        return StatusCode::FORBIDDEN.into_response();
    };
    // Admission and removal share this lock: teardown cannot miss a late upgrade.
    let (control, permit, token) = {
        let registry = state.controls.lock().expect("web registry poisoned");
        if registry.closing {
            return StatusCode::FORBIDDEN.into_response();
        }
        let Some(control) = registry.controls.get(id).cloned() else {
            return StatusCode::FORBIDDEN.into_response();
        };
        if *control.stop.borrow() {
            return StatusCode::FORBIDDEN.into_response();
        }
        let Ok(permit) = Arc::clone(&control.admitted).try_acquire_owned() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let token = control.tasks.token();
        (control, permit, token)
    };
    upgrade_limits(ws)
        .on_upgrade(move |socket| async move {
            let _tracking = token;
            let _admission = permit;
            stream_socket(socket, state, control).await;
        })
        .into_response()
}

fn close(code: u16, reason: &'static str) -> Message {
    Message::Close(Some(CloseFrame {
        code,
        reason: reason.into(),
    }))
}

async fn control_socket(socket: WebSocket, state: WebState) {
    let id = uuid::Uuid::new_v4().to_string();
    let session = Arc::new(Mutex::new(dispatch::Session {
        fingerprint: None,
        authorized: true,
        subscriber_id: uuid::Uuid::new_v4().to_string(),
        folders: HashSet::new(),
        folder_aliases: Default::default(),
        next_events: 0,
        events: None,
    }));
    let events = state.context.host_events.subscribe();
    let (stop, mut stop_rx) = watch::channel(false);
    let control = Arc::new(Control {
        session,
        admitted: Arc::new(Semaphore::new(MAX_STREAMS_PER_CONNECTION as usize)),
        tasks: TaskTracker::new(),
        stop,
        live_ids: Arc::new(StdMutex::new(HashSet::new())),
        close_code: AtomicU16::new(close_code::NORMAL),
    });
    {
        let mut registry = state.controls.lock().expect("web registry poisoned");
        if registry.closing {
            return;
        }
        registry.controls.insert(id.clone(), Arc::clone(&control));
    }
    let (mut sink, stream) = socket.split();
    let ready = serde_json::to_string(&Ready {
        ready: ReadyId { connection_id: &id },
    })
    .expect("ready is serializable");
    let ready_sent = if *stop_rx.borrow() {
        false
    } else {
        tokio::select! {
            _ = stop_rx.changed() => false,
            result = timeout(IO_TIMEOUT, sink.send(Message::Text(ready.into()))) => matches!(result, Ok(Ok(()))),
        }
    };
    let writer = if ready_sent {
        let (tx, rx) = mpsc::channel(16);
        let writer_control = Arc::clone(&control);
        let writer = tokio::spawn(async move {
            write_control(sink, rx, writer_control, stop_rx).await;
        });
        let code = read_control(stream, events, tx, &state, &control).await;
        if code != close_code::NORMAL {
            control.close_code.store(code, Ordering::Relaxed);
        }
        Some(writer)
    } else {
        None
    };
    // Remove before signaling children, and prevent a racing stream from escaping the drain.
    {
        let mut registry = state.controls.lock().expect("web registry poisoned");
        registry.controls.remove(&id);
        control.tasks.close();
        let _ = control.stop.send(true);
    }
    if let Some(writer) = writer {
        if let Err(error) = writer.await {
            tracing::warn!(%error, "web writer task failed");
        }
    }
    control.tasks.wait().await;
    cleanup_session(&state.host, &state.context, &control.session).await;
}

async fn write_control(
    mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
    mut rx: mpsc::Receiver<Outgoing>,
    control: Arc<Control>,
    mut stop: watch::Receiver<bool>,
) {
    loop {
        if *stop.borrow() {
            break;
        }
        tokio::select! {
            biased;
            _ = stop.changed() => break,
            outgoing = rx.recv() => {
                let Some(outgoing) = outgoing else { break; };
                let result = timeout(IO_TIMEOUT, sink.send(outgoing.message)).await;
                if let Some(id) = outgoing.id { control.live_ids.lock().expect("request IDs poisoned").remove(&id); }
                if !matches!(result, Ok(Ok(()))) { break; }
            }
        }
    }
    let _ = control.stop.send(true);
    let code = control.close_code.load(Ordering::Relaxed);
    let _ = timeout(IO_TIMEOUT, sink.send(close(code, "closing"))).await;
}

async fn read_control(
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    mut events: broadcast::Receiver<Arc<sworm_protocol::rpc::HostEventWire>>,
    tx: mpsc::Sender<Outgoing>,
    state: &WebState,
    control: &Arc<Control>,
) -> u16 {
    let mut stop = control.stop.subscribe();
    if *stop.borrow() {
        return close_code::NORMAL;
    }
    let mut heartbeat = interval_at(Instant::now() + HEARTBEAT_INTERVAL, HEARTBEAT_INTERVAL);
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sequence = 0_u32;
    let mut awaiting_pong = None;
    let expiry = tokio::time::sleep(IO_TIMEOUT);
    tokio::pin!(expiry);
    loop {
        tokio::select! {
            _ = stop.changed() => break close_code::NORMAL,
            _ = &mut expiry, if awaiting_pong.is_some() => break close_code::AWAY,
            _ = heartbeat.tick() => {
                if awaiting_pong.is_none() {
                    sequence = sequence.wrapping_add(1);
                    let message = Message::Text(format!(r#"{{"ping":{sequence}}}"#).into());
                    if tx.try_send(Outgoing { message, id: None, _admission: None }).is_err() {
                        break close_code::ERROR;
                    }
                    awaiting_pong = Some(sequence);
                    expiry.as_mut().reset(Instant::now() + IO_TIMEOUT);
                }
            },
            event = events.recv() => match event {
                Ok(event) => {
                    if events::claimed(&control.session, &event).await {
                        let Ok(text) = serde_json::to_string(&EventEnvelope { event: &event }) else { break close_code::ERROR; };
                        if text.len() > MAX_FRAME_BYTES { break close_code::SIZE; }
                        if tx.send(Outgoing { message: Message::Text(text.into()), id: None, _admission: None }).await.is_err() { break close_code::NORMAL; }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) | Err(broadcast::error::RecvError::Closed) => break close_code::ERROR,
            },
            incoming = stream.next() => {
                let message = match incoming { Some(Ok(message)) => message, Some(Err(_)) => break close_code::SIZE, None => break close_code::NORMAL };
                let text = match message {
                    Message::Text(text) => text,
                    Message::Ping(_) | Message::Pong(_) => continue,
                    Message::Close(_) => break close_code::NORMAL,
                    Message::Binary(_) => break close_code::POLICY,
                };
                if text.len() > MAX_FRAME_BYTES { break close_code::SIZE; }
                let envelope = match serde_json::from_str::<Incoming>(&text) {
                    Ok(Incoming { id: None, request: None, pong: Some(pong) }) => {
                        if awaiting_pong == Some(pong) {
                            awaiting_pong = None;
                        } else {
                            break close_code::POLICY;
                        }
                        continue;
                    }
                    Ok(Incoming { id: Some(id), request: Some(request), pong: None }) => RequestEnvelope { id, request },
                    _ => break close_code::POLICY,
                };
                let Ok(admission) = Arc::clone(&control.admitted).try_acquire_owned() else { break close_code::POLICY; };
                if !control.live_ids.lock().expect("request IDs poisoned").insert(envelope.id) { break close_code::POLICY; }
                let host = Arc::clone(&state.host);
                let context = Arc::clone(&state.context);
                let session = Arc::clone(&control.session);
                let requests = Arc::clone(&state.requests);
                let tx = tx.clone();
                let reply_control = Arc::clone(control);
                control.tasks.spawn(async move {
                    let Ok(execution) = requests.acquire_owned().await else { return; };
                    let response = dispatch::handle(&host, &context, &session, envelope.request).await;
                    drop(execution);
                    match serde_json::to_string(&ReplyEnvelope { id: envelope.id, response: &response }) {
                        Ok(text) if text.len() <= MAX_FRAME_BYTES => {
                            let _ = tx.send(Outgoing { message: Message::Text(text.into()), id: Some(envelope.id), _admission: Some(admission) }).await;
                        }
                        Ok(_) => {
                            reply_control.close_code.store(close_code::SIZE, Ordering::Relaxed);
                            let _ = reply_control.stop.send(true);
                        }
                        Err(_) => {
                            reply_control.close_code.store(close_code::ERROR, Ordering::Relaxed);
                            let _ = reply_control.stop.send(true);
                        }
                    }
                });
            }
        }
    }
}

async fn stream_socket(mut socket: WebSocket, state: WebState, control: Arc<Control>) {
    let mut stop = control.stop.subscribe();
    if *stop.borrow() {
        return;
    }
    let first = tokio::select! {
        _ = stop.changed() => return,
        result = timeout(IO_TIMEOUT, async {
            loop {
                match socket.recv().await {
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    message => break message,
                }
            }
        }) => result,
    };
    let open = match first {
        Ok(Some(Ok(Message::Binary(body)))) if body.len() <= MAX_REQUEST_FRAME_BYTES => {
            serde_json::from_slice::<Open>(&body)
        }
        Ok(Some(Ok(Message::Binary(_)))) => {
            let _ = timeout(
                IO_TIMEOUT,
                socket.send(close(close_code::SIZE, "stream open too large")),
            )
            .await;
            return;
        }
        Ok(Some(Ok(Message::Text(text)))) if text.len() > MAX_REQUEST_FRAME_BYTES => {
            let _ = timeout(
                IO_TIMEOUT,
                socket.send(close(close_code::SIZE, "stream open too large")),
            )
            .await;
            return;
        }
        Ok(Some(Err(_))) => {
            let _ = timeout(
                IO_TIMEOUT,
                socket.send(close(close_code::SIZE, "stream frame too large")),
            )
            .await;
            return;
        }
        _ => {
            let _ = timeout(
                IO_TIMEOUT,
                socket.send(close(close_code::POLICY, "binary stream open required")),
            )
            .await;
            return;
        }
    };
    let Ok(open) = open else {
        let _ = timeout(
            IO_TIMEOUT,
            socket.send(close(close_code::POLICY, "invalid stream open")),
        )
        .await;
        return;
    };
    if matches!(open, Open::Rpc(_) | Open::Events) {
        let _ = timeout(
            IO_TIMEOUT,
            socket.send(close(close_code::POLICY, "unsupported stream open")),
        )
        .await;
        return;
    }
    if *stop.borrow() {
        return;
    }
    let (sink, stream) = socket.split();
    let (stream_stop, stream_rx) = watch::channel(None);
    let mut writer = StreamWriter::web(sink, stream_rx);
    let reader = StreamReader::web(stream, stream_stop);
    match open {
        Open::Pty { run_id, cursor } => {
            pty_stream::run(
                state.host,
                state.context,
                run_id,
                cursor,
                writer,
                reader,
                stop,
            )
            .await;
        }
        Open::Lsp { session_id } => {
            let owner = control.session.lock().await.subscriber_id.clone();
            lsp_stream::run(
                state.host,
                state.context,
                session_id,
                owner,
                writer,
                reader,
                stop,
            )
            .await;
        }
        Open::FileRead {
            project_path,
            file_path,
            version,
        } => {
            if let Err(error) = dispatch::claim_file_read(
                &state.host,
                &state.context,
                &control.session,
                &project_path,
            )
            .await
            {
                let _ = timeout(
                    IO_TIMEOUT,
                    writer.write_json(&FileReadDown::Error { error }),
                )
                .await;
                let _ = timeout(IO_TIMEOUT, writer.finish()).await;
                return;
            }
            file_stream::run(
                state.host,
                project_path,
                file_path,
                version,
                writer,
                reader,
                stop,
            )
            .await;
        }
        Open::Rpc(_) | Open::Events => unreachable!(),
    }
}
