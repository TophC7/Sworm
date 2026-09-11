use crate::router::RouterInner;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
};
use sworm_core::{
    errors::ApiError,
    events::EventSink,
    services::pty::{PtyEventSink, RunBackend},
    Host,
};
use sworm_protocol::{
    pty::PtyEvent,
    rpc::{Open, PtyCursor, PtyDown, PtyUp, Request},
};
use sworm_remote::wire::{read_tagged_frame, write_frame, write_raw_frame, Frame};
use tokio::{sync::mpsc, task::JoinHandle, time::sleep};

use crate::router::{INITIAL_RECONNECT_DELAY, MAX_RECONNECT_DELAY};

const INPUT_QUEUE_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RemoteRunKind {
    Session,
    Task,
}

impl RemoteRunKind {
    pub(crate) fn stop_request(self, run_id: String) -> Request {
        match self {
            Self::Session => Request::SessionStop { run_id },
            Self::Task => Request::TasksStop { run_id },
        }
    }
}

enum Upstream {
    Bytes(Vec<u8>),
    Control(PtyUp),
}

struct RemoteRun {
    server: String,
    kind: RemoteRunKind,
    generation: u64,
    // Completion and detach retain only routing metadata for later status/stop.
    stream: Option<RemoteRunStream>,
}

struct RemoteRunStream {
    stopping: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

pub(crate) struct RemoteRunInfo {
    pub server: String,
    pub kind: RemoteRunKind,
    pub generation: u64,
    pub stopping: Option<Arc<AtomicBool>>,
}

pub(crate) struct RemoteRunService {
    runs: Mutex<HashMap<String, RemoteRun>>,
    next_generation: AtomicU64,
}

impl RemoteRunService {
    pub fn new() -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
        }
    }

    pub fn ensure_startable(&self, run_id: &str) -> Result<(), ApiError> {
        if self
            .runs
            .lock()
            .get(run_id)
            .and_then(|run| run.stream.as_ref())
            .is_some_and(|stream| stream.stopping.load(Ordering::Acquire))
        {
            return Err(ApiError::Pty(format!(
                "remote PTY session is stopping: {run_id}"
            )));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adopt(
        &self,
        router: Weak<RouterInner>,
        host: &Arc<Host>,
        run_id: String,
        server: String,
        kind: RemoteRunKind,
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
        owner_id: Option<String>,
    ) -> Result<(), ApiError> {
        if host.pty.run_state(&run_id).is_some() {
            return Err(ApiError::Pty(format!(
                "remote PTY session already exists: {run_id}"
            )));
        }
        self.ensure_startable(&run_id)?;
        let stale = self.runs.lock().remove(&run_id);
        if let Some(stream) = stale.and_then(|run| run.stream) {
            stream.task.abort();
        }
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let stopping = Arc::new(AtomicBool::new(false));
        let cursor = PtyCursor::default();
        let (up, up_rx) = mpsc::channel(INPUT_QUEUE_CAPACITY);
        let backend = WireBackend {
            router: router.clone(),
            run_id: run_id.clone(),
            kind,
            generation,
            stopping: Arc::clone(&stopping),
            up,
        };
        let sink = host
            .pty
            .adopt(run_id.clone(), Box::new(backend), output, events, owner_id)
            .map_err(ApiError::Pty)?;

        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let task_run_id = run_id.clone();
        let task_server = server.clone();
        let task_stopping = Arc::clone(&stopping);
        let task = tokio::spawn(async move {
            if start_rx.await.is_err() {
                return;
            }
            run_stream(
                router,
                task_run_id,
                task_server,
                generation,
                cursor,
                task_stopping,
                sink,
                up_rx,
            )
            .await;
        });

        let _old = self.runs.lock().insert(
            run_id.to_string(),
            RemoteRun {
                server: server.to_string(),
                kind,
                generation,
                stream: Some(RemoteRunStream { stopping, task }),
            },
        );
        debug_assert!(_old.is_none(), "host adoption rejected duplicate run ids");
        let _ = start_tx.send(());
        Ok(())
    }

    pub fn info(&self, run_id: &str) -> Option<RemoteRunInfo> {
        self.runs.lock().get(run_id).map(|run| RemoteRunInfo {
            server: run.server.clone(),
            kind: run.kind,
            generation: run.generation,
            stopping: run
                .stream
                .as_ref()
                .map(|stream| Arc::clone(&stream.stopping)),
        })
    }

    pub fn server_for(&self, run_id: &str) -> Option<String> {
        self.runs.lock().get(run_id).map(|run| run.server.clone())
    }

    pub fn cancel(&self, run_id: &str, generation: u64) {
        let run = {
            let mut runs = self.runs.lock();
            if !runs
                .get(run_id)
                .is_some_and(|run| run.generation == generation)
            {
                return;
            }
            runs.remove(run_id)
        };
        if let Some(stream) = run.and_then(|run| run.stream) {
            stream.task.abort();
        }
    }

    fn finish_stream(&self, run_id: &str, generation: u64) {
        let stream = {
            let mut runs = self.runs.lock();
            runs.get_mut(run_id)
                .filter(|run| run.generation == generation)
                .and_then(|run| run.stream.take())
        };
        drop(stream);
    }
}

impl Drop for RemoteRunService {
    fn drop(&mut self) {
        for (_, run) in self.runs.get_mut().drain() {
            if let Some(stream) = run.stream {
                stream.task.abort();
            }
        }
    }
}

struct WireBackend {
    router: Weak<RouterInner>,
    run_id: String,
    kind: RemoteRunKind,
    generation: u64,
    stopping: Arc<AtomicBool>,
    up: mpsc::Sender<Upstream>,
}

impl RunBackend for WireBackend {
    fn write(&self, data: &[u8]) -> Result<(), String> {
        self.up
            .try_send(Upstream::Bytes(data.to_vec()))
            .map_err(|error| format!("remote PTY input unavailable: {error}"))
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.up
            .try_send(Upstream::Control(PtyUp::Resize { cols, rows }))
            .map_err(|error| format!("remote PTY input unavailable: {error}"))
    }

    fn kill(&self) -> Result<(), String> {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let Some(router) = self.router.upgrade() else {
            return Ok(());
        };
        let run_id = self.run_id.clone();
        let kind = self.kind;
        let generation = self.generation;
        tauri::async_runtime::spawn(async move {
            if let Err(error) = router.stop_backend(&run_id, kind).await {
                tracing::warn!(%run_id, %error, "failed to stop remote PTY");
                if let Some(server) = router.remote_runs.server_for(&run_id) {
                    router.remember_failed_stop(&server, &run_id, kind, &error);
                }
            }
            router.remote_runs.cancel(&run_id, generation);
        });
        Ok(())
    }

    fn detaches_on_shutdown(&self) -> bool {
        true
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_stream(
    router: Weak<RouterInner>,
    run_id: String,
    server: String,
    generation: u64,
    mut cursor: PtyCursor,
    stopping: Arc<AtomicBool>,
    sink: PtyEventSink,
    mut up_rx: mpsc::Receiver<Upstream>,
) {
    let mut pending = None;
    let mut reconnect_delay = INITIAL_RECONNECT_DELAY;
    loop {
        if stopping.load(Ordering::Acquire) {
            break;
        }
        let Some(router_now) = router.upgrade() else {
            break;
        };
        let client = match router_now.client(&server).await {
            Ok(client) => client,
            Err(error) => {
                tracing::warn!(%server, %run_id, %error, "remote PTY reconnect failed");
                drop(router_now);
                sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
                continue;
            }
        };
        drop(router_now);

        let (mut send, mut recv) = match client
            .open_stream(Open::Pty {
                run_id: run_id.clone(),
                cursor,
            })
            .await
        {
            Ok(streams) => streams,
            Err(error) => {
                evict_failed(&router, &server, &client).await;
                tracing::warn!(%server, %run_id, %error, "remote PTY stream open failed");
                sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
                continue;
            }
        };
        reconnect_delay = INITIAL_RECONNECT_DELAY;
        if let Some(message) = pending.take() {
            if send_upstream(&mut send, &message).await.is_err() {
                pending = Some(message);
                evict_failed(&router, &server, &client).await;
                sleep(reconnect_delay).await;
                reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
                continue;
            }
        }

        let mut exited = false;
        loop {
            tokio::select! {
                message = up_rx.recv() => {
                    let Some(message) = message else {
                        let _ = send.finish();
                        if let Some(router) = router.upgrade() {
                            router.remote_runs.finish_stream(&run_id, generation);
                        }
                        exited = true;
                        break;
                    };
                    if send_upstream(&mut send, &message).await.is_err() {
                        pending = Some(message);
                        break;
                    }
                }
                frame = read_tagged_frame::<PtyDown>(&mut recv) => {
                    match frame {
                        Ok(Frame::Raw(body)) => consume_output(&run_id, &mut cursor, &sink, body),
                        Ok(Frame::Json(PtyDown::Gap { lost_bytes })) => {
                            cursor.output_offset =
                                cursor.output_offset.saturating_add(lost_bytes);
                            sink.emit(PtyEvent::Error {
                                run_id: run_id.clone(),
                                message: format!("{lost_bytes} bytes of output were lost while disconnected"),
                            });
                        }
                        Ok(Frame::Json(PtyDown::Event { sequence, event })) => {
                            if matches!(&event, PtyEvent::Synced { .. }) {
                                cursor.event_sequence = cursor.event_sequence.max(sequence);
                                // The wire sequence belongs to the daemon's sink;
                                // the local sink restamps it for its subscriber.
                                sink.emit_sync_marker();
                                continue;
                            }
                            if sequence <= cursor.event_sequence {
                                continue;
                            }
                            cursor.event_sequence = sequence;
                            if let PtyEvent::Exit { code, .. } = &event {
                                if let Some(router_now) = router.upgrade() {
                                    if let Err(error) =
                                        router_now.host.pty.complete_adopted(&run_id, *code)
                                    {
                                        tracing::warn!(
                                            %run_id,
                                            %error,
                                            "failed to complete adopted PTY"
                                        );
                                    }
                                    router_now
                                        .remote_runs
                                        .finish_stream(&run_id, generation);
                                }
                                exited = true;
                                break;
                            }
                            sink.emit(event);
                        }
                        Ok(Frame::Json(PtyDown::Closed { error })) => {
                            // The daemon can never serve this run again, so the
                            // run ends here instead of reconnecting forever.
                            let message = ApiError::from(error).to_string();
                            tracing::warn!(%server, %run_id, %message, "remote PTY run is gone");
                            sink.emit(PtyEvent::Error {
                                run_id: run_id.clone(),
                                message,
                            });
                            if let Some(router_now) = router.upgrade() {
                                if let Err(error) =
                                    router_now.host.pty.complete_adopted(&run_id, None)
                                {
                                    tracing::warn!(%run_id, %error, "failed to complete adopted PTY");
                                }
                                router_now.remote_runs.finish_stream(&run_id, generation);
                            }
                            exited = true;
                            break;
                        }
                        Err(error) => {
                            tracing::warn!(%server, %run_id, %error, "remote PTY stream lost");
                            break;
                        }
                    }
                }
                _ = client.closed() => break,
            }
        }
        if exited || stopping.load(Ordering::Acquire) {
            break;
        }
        evict_failed(&router, &server, &client).await;
        sleep(reconnect_delay).await;
        reconnect_delay = (reconnect_delay * 2).min(MAX_RECONNECT_DELAY);
    }
}
async fn send_upstream(
    send: &mut quinn::SendStream,
    message: &Upstream,
) -> Result<(), sworm_remote::RemoteError> {
    match message {
        Upstream::Bytes(bytes) => write_raw_frame(send, bytes).await,
        Upstream::Control(control) => write_frame(send, control).await,
    }
}

fn consume_output(run_id: &str, cursor: &mut PtyCursor, sink: &PtyEventSink, mut body: Vec<u8>) {
    if body.len() < 8 {
        sink.emit(PtyEvent::Error {
            run_id: run_id.to_owned(),
            message: "remote PTY sent an output frame without an offset".to_owned(),
        });
        return;
    }
    let start = u64::from_be_bytes(body[..8].try_into().expect("eight-byte slice"));
    let output_len = body.len() - 8;
    let end = start.saturating_add(output_len as u64);
    if end <= cursor.output_offset {
        return;
    }
    if start > cursor.output_offset {
        let lost = start - cursor.output_offset;
        sink.emit(PtyEvent::Error {
            run_id: run_id.to_owned(),
            message: format!("{lost} bytes of output were lost while disconnected"),
        });
        cursor.output_offset = start;
    }
    let skip = usize::try_from(cursor.output_offset.saturating_sub(start))
        .unwrap_or(usize::MAX)
        .min(output_len);
    body.drain(..8 + skip);
    cursor.output_offset = end;
    if !body.is_empty() {
        sink.emit_output(body);
    }
}

async fn evict_failed(
    router: &Weak<RouterInner>,
    server: &str,
    client: &Arc<sworm_remote::RemoteClient>,
) {
    if let Some(router) = router.upgrade() {
        router.evict(server, client).await;
    }
}
