use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

const PTY_READ_BUF_SIZE: usize = 64 * 1024;
const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
const RETAINED_OUTPUT_CAP: usize = 8 * 1024 * 1024;

/// Events emitted over the lifecycle channel. `run_id` is the ephemeral
/// PTY identity minted by the frontend for one spawn; the durable tab
/// identity never reaches this layer.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PtyEvent {
    Started {
        run_id: String,
        pid: Option<u32>,
    },
    Exit {
        run_id: String,
        code: Option<i32>,
    },
    Error {
        run_id: String,
        message: String,
    },
    /// A provider-side resume identity was discovered for a run after
    /// spawn (Codex thread id, Antigravity conversation id, OMP session id).
    ResumeTokenBound {
        run_id: String,
        token: String,
    },
}

#[derive(Clone)]
struct PtyChannels {
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
}

enum SubscriberState {
    Active(PtyChannels),
    Paused,
}

#[derive(Clone)]
enum Payload {
    Output(Vec<u8>),
    Event(PtyEvent),
}

impl Payload {
    fn send(self, channels: &PtyChannels) -> Result<(), String> {
        match self {
            Self::Output(bytes) => channels
                .output
                .send(bytes)
                .map_err(|_| "PTY output channel closed while attaching".to_string()),
            Self::Event(event) => channels
                .events
                .send(event)
                .map_err(|_| "PTY event channel closed while attaching".to_string()),
        }
    }
}

struct Retained {
    sequence: u64,
    payload: Payload,
    delivered_to_target: bool,
}

struct PtyStreamState {
    subscriber: SubscriberState,
    original: Option<PtyChannels>,
    retained: Vec<Retained>,
    retained_bytes: usize,
    overflow_warned: bool,
    last_dispatched: u64,
    completed: bool,
}

impl PtyStreamState {
    fn retain(&mut self, sequence: u64, payload: Payload, delivered: bool) {
        if let Payload::Output(bytes) = &payload {
            self.retained_bytes += bytes.len();
        }
        self.retained.push(Retained {
            sequence,
            payload,
            delivered_to_target: delivered,
        });
        if self.retained_bytes > RETAINED_OUTPUT_CAP {
            if !self.overflow_warned {
                warn!("PTY retained output exceeded cap; dropping oldest output bytes");
                self.overflow_warned = true;
            }
            for entry in &mut self.retained {
                if self.retained_bytes <= RETAINED_OUTPUT_CAP {
                    break;
                }
                if let Payload::Output(bytes) = &mut entry.payload {
                    self.retained_bytes -= bytes.len();
                    *bytes = Vec::new();
                }
            }
        }
    }

    fn clear_retained(&mut self) {
        self.retained.clear();
        self.retained_bytes = 0;
        self.overflow_warned = false;
    }
}

/// Thread-safe lifecycle event handle which follows the PTY's currently
/// attached webview and queues events while no webview is attached.
#[derive(Clone)]
pub struct PtyEventSink {
    sequence: Arc<AtomicU64>,
    state: Arc<Mutex<PtyStreamState>>,
}

impl PtyEventSink {
    fn new(output: tauri::ipc::Channel<Vec<u8>>, events: tauri::ipc::Channel<PtyEvent>) -> Self {
        Self {
            sequence: Arc::new(AtomicU64::new(1)),
            state: Arc::new(Mutex::new(PtyStreamState {
                subscriber: SubscriberState::Active(PtyChannels { output, events }),
                original: None,
                retained: Vec::new(),
                retained_bytes: 0,
                overflow_warned: false,
                last_dispatched: 0,
                completed: false,
            })),
        }
    }

    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    fn emit_output(&self, bytes: Vec<u8>) {
        self.emit_payload(Payload::Output(bytes));
    }

    pub fn emit(&self, event: PtyEvent) {
        self.emit_payload(Payload::Event(event));
    }

    fn emit_payload(&self, payload: Payload) {
        let mut state = self.state.lock();
        let sequence = self.next_sequence();
        if matches!(&state.subscriber, SubscriberState::Paused) {
            state.retain(sequence, payload, false);
            return;
        }
        let transferring = state.original.is_some();
        if transferring {
            state.retain(sequence, payload.clone(), true);
        }
        let SubscriberState::Active(channels) = &state.subscriber else {
            unreachable!("paused subscriber handled above");
        };
        match payload.send(channels) {
            Ok(()) => state.last_dispatched = sequence,
            Err(err) => {
                state.subscriber = SubscriberState::Paused;
                if transferring {
                    if let Some(entry) = state.retained.last_mut() {
                        entry.delivered_to_target = false;
                    }
                } else {
                    warn!("PTY subscriber channel closed: {}", err);
                }
            }
        }
    }

    fn complete(&self, event: PtyEvent) -> bool {
        self.emit(event);
        let mut state = self.state.lock();
        state.completed = true;
        state.original.is_some() || matches!(&state.subscriber, SubscriberState::Paused)
    }

    fn pause(&self) -> u64 {
        let mut state = self.state.lock();
        let channels = match &state.subscriber {
            SubscriberState::Active(channels) => Some(channels.clone()),
            SubscriberState::Paused => None,
        };
        if let Some(channels) = channels {
            if state.original.is_none() {
                state.original = Some(channels);
            }
            state.subscriber = SubscriberState::Paused;
        }
        state.last_dispatched
    }

    fn attach(&self, channels: PtyChannels) -> Result<u64, String> {
        let mut state = self.state.lock();
        state.subscriber = SubscriberState::Active(channels.clone());

        for index in 0..state.retained.len() {
            if state.retained[index].delivered_to_target {
                continue;
            }
            if let Err(err) = state.retained[index].payload.clone().send(&channels) {
                state.subscriber = SubscriberState::Paused;
                return Err(err);
            }
            state.retained[index].delivered_to_target = true;
            state.last_dispatched = state.retained[index].sequence;
        }
        if state.original.is_none() {
            state.clear_retained();
        }

        Ok(self.sequence.load(Ordering::Acquire).saturating_sub(1))
    }

    fn resume_original(&self) -> Result<u64, String> {
        let mut state = self.state.lock();
        let channels = state
            .original
            .take()
            .ok_or_else(|| "PTY has no original subscriber to restore".to_string())?;
        state.subscriber = SubscriberState::Active(channels.clone());
        // Target delivery flags do not apply to the restored original subscriber.
        for entry in &mut state.retained {
            entry.delivered_to_target = false;
        }
        for index in 0..state.retained.len() {
            if let Err(err) = state.retained[index].payload.clone().send(&channels) {
                state.subscriber = SubscriberState::Paused;
                return Err(err);
            }
            state.retained[index].delivered_to_target = true;
            state.last_dispatched = state.retained[index].sequence;
        }
        state.clear_retained();
        Ok(self.sequence.load(Ordering::Acquire).saturating_sub(1))
    }

    fn commit_transfer(&self) {
        let mut state = self.state.lock();
        state.original = None;
        state.clear_retained();
    }

    fn is_completed(&self) -> bool {
        self.state.lock().completed
    }
}

/// A single live PTY session.
struct LivePty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    shutdown: Arc<AtomicBool>,
    finalized: Arc<AtomicBool>,
    runtime_id: String,
    event_sink: PtyEventSink,
    owner_window: Option<String>,
}

/// PTY service managing active sessions and completed detached sessions whose
/// queued tail has not yet been delivered.
pub struct PtyService {
    sessions: Arc<Mutex<HashMap<String, LivePty>>>,
}

impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a new PTY running the given command.
    /// If a PTY already exists for this run_id, it is killed first.
    pub fn spawn(
        &self,
        run_id: String,
        cmd: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        cols: u16,
        rows: u16,
        output_channel: tauri::ipc::Channel<Vec<u8>>,
        event_channel: tauri::ipc::Channel<PtyEvent>,
        owner_window: Option<String>,
        on_exit: Option<Box<dyn FnOnce(&str, Option<i32>) + Send>>,
    ) -> Result<PtyEventSink, String> {
        if self.sessions.lock().contains_key(&run_id) {
            info!("Killing existing PTY for {} before respawn", run_id);
            let _ = self.kill(&run_id);
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("Failed to open PTY: {}", e))?;

        let mut cmd_builder = CommandBuilder::new(cmd);
        for arg in args {
            cmd_builder.arg(arg);
        }
        if let Some(cwd) = cwd {
            cmd_builder.cwd(cwd);
        }
        if let Some(env_map) = env {
            for (key, value) in env_map {
                cmd_builder.env(key, value);
            }
        }

        let mut child = pair
            .slave
            .spawn_command(cmd_builder)
            .map_err(|e| format!("Failed to spawn process: {}", e))?;
        let pid = child.process_id();
        let killer = child.clone_killer();
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| format!("Failed to get PTY writer: {}", e))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));
        let runtime_id = uuid::Uuid::new_v4().to_string();
        let event_sink = PtyEventSink::new(output_channel, event_channel);

        self.sessions.lock().insert(
            run_id.clone(),
            LivePty {
                master: pair.master,
                writer,
                killer,
                shutdown: shutdown.clone(),
                finalized: finalized.clone(),
                runtime_id: runtime_id.clone(),
                event_sink: event_sink.clone(),
                owner_window,
            },
        );

        event_sink.emit(PtyEvent::Started {
            run_id: run_id.clone(),
            pid,
        });

        let sid_for_thread = run_id.clone();
        let sessions_for_thread = Arc::clone(&self.sessions);
        let sink_for_thread = event_sink.clone();

        std::thread::Builder::new()
            .name(format!("pty-reader-{}", &run_id))
            .spawn(move || {
                let mut buf = [0u8; PTY_READ_BUF_SIZE];
                let pending: Arc<Mutex<Vec<u8>>> =
                    Arc::new(Mutex::new(Vec::with_capacity(64 * 1024)));
                let flusher_stop = Arc::new(AtomicBool::new(false));

                let pending_for_flusher = Arc::clone(&pending);
                let flusher_stop_inner = Arc::clone(&flusher_stop);
                let sink_for_flusher = sink_for_thread.clone();
                let flusher_handle = std::thread::Builder::new()
                    .name(format!("pty-flusher-{}", &sid_for_thread))
                    .spawn(move || loop {
                        std::thread::sleep(OUTPUT_FLUSH_INTERVAL);
                        let bytes = {
                            let mut pending = pending_for_flusher.lock();
                            (!pending.is_empty()).then(|| std::mem::take(&mut *pending))
                        };
                        if let Some(bytes) = bytes {
                            sink_for_flusher.emit_output(bytes);
                        }
                        if flusher_stop_inner.load(Ordering::Relaxed) {
                            break;
                        }
                    })
                    .map(Some)
                    .unwrap_or_else(|err| {
                        error!("Failed to spawn PTY flusher thread: {}", err);
                        None
                    });

                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        info!("PTY shutdown requested for {}", sid_for_thread);
                        break;
                    }
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            info!("PTY reader EOF for {}", sid_for_thread);
                            break;
                        }
                        Ok(n) => pending.lock().extend_from_slice(&buf[..n]),
                        Err(err) => {
                            if shutdown.load(Ordering::Relaxed) {
                                info!(
                                    "PTY read loop stopped during shutdown for {}",
                                    sid_for_thread
                                );
                            } else {
                                error!("PTY read error for {}: {}", sid_for_thread, err);
                                sink_for_thread.emit(PtyEvent::Error {
                                    run_id: sid_for_thread.clone(),
                                    message: err.to_string(),
                                });
                            }
                            break;
                        }
                    }
                }

                // Flush the output tail before assigning the Exit event sequence.
                flusher_stop.store(true, Ordering::Release);
                if let Some(handle) = flusher_handle {
                    if let Err(err) = handle.join() {
                        warn!("PTY flusher thread panicked: {:?}", err);
                    }
                }

                let exit_status = child.wait();
                let exit_code = exit_status
                    .as_ref()
                    .ok()
                    .map(|status| status.exit_code() as i32);
                if let Err(err) = exit_status {
                    warn!("Failed waiting for PTY child {}: {}", sid_for_thread, err);
                }

                if finalized.swap(true, Ordering::AcqRel) {
                    info!("Skipping duplicate PTY finalization for {}", sid_for_thread);
                    return;
                }
                if let Some(callback) = on_exit {
                    callback(&sid_for_thread, exit_code);
                }

                let retain = sink_for_thread.complete(PtyEvent::Exit {
                    run_id: sid_for_thread.clone(),
                    code: exit_code,
                });
                if retain {
                    info!(
                        "PTY {} exited while detached; retaining queued tail",
                        sid_for_thread
                    );
                    return;
                }

                let mut sessions = sessions_for_thread.lock();
                let remove_current = sessions
                    .get(&sid_for_thread)
                    .map(|live| live.runtime_id == runtime_id)
                    .unwrap_or(false);
                if remove_current {
                    sessions.remove(&sid_for_thread);
                }
            })
            .map_err(|e| format!("Failed to spawn reader thread: {}", e))?;

        Ok(event_sink)
    }

    /// Atomically detach webview channels and return the last delivered sequence.
    pub fn pause(&self, run_id: &str) -> Result<u64, String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        Ok(sink.pause())
    }

    /// Attach replacement channels and deliver everything queued while detached.
    /// A completed session stays registered until the transfer commits or
    /// aborts so the broker can still resolve ownership.
    pub fn attach(
        &self,
        run_id: &str,
        output: tauri::ipc::Channel<Vec<u8>>,
        events: tauri::ipc::Channel<PtyEvent>,
    ) -> Result<u64, String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        sink.attach(PtyChannels { output, events })
    }

    /// Abort a transfer and restore the channels detached by `pause`.
    pub fn resume_original(&self, run_id: &str) -> Result<(), String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        sink.resume_original()?;
        if sink.is_completed() {
            self.sessions.lock().remove(run_id);
        }
        Ok(())
    }

    /// Commit a transfer to its new owning window, reaping the session if it
    /// already completed while detached.
    pub fn transfer_owner(&self, run_id: &str, new_owner_window: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock();
        let live = sessions
            .get_mut(run_id)
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        live.owner_window = Some(new_owner_window.to_string());
        live.event_sink.commit_transfer();
        let completed = live.event_sink.is_completed();
        if completed {
            sessions.remove(run_id);
        }
        Ok(())
    }

    /// Discard rollback data once a transfer no longer needs its original sink.
    pub fn commit_transfer(&self, run_id: &str) {
        let mut sessions = self.sessions.lock();
        if let Some(live) = sessions.get(run_id) {
            live.event_sink.commit_transfer();
            if live.event_sink.is_completed() {
                sessions.remove(run_id);
            }
        }
    }

    /// Write data to the PTY's stdin.
    pub fn write(&self, run_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock();
        let live = sessions
            .get_mut(run_id)
            .ok_or_else(|| format!("No active PTY session: {}", run_id))?;
        live.writer
            .write_all(data)
            .map_err(|e| format!("PTY write failed: {}", e))?;
        live.writer
            .flush()
            .map_err(|e| format!("PTY flush failed: {}", e))
    }

    /// Resize the PTY.
    pub fn resize(&self, run_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock();
        let live = sessions
            .get(run_id)
            .ok_or_else(|| format!("No active PTY session: {}", run_id))?;
        live.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("PTY resize failed: {}", e))
    }

    /// Kill the PTY process and clean up.
    pub fn kill(&self, run_id: &str) -> Result<(), String> {
        let live = self.sessions.lock().remove(run_id);
        if let Some(mut live) = live {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            live.killer
                .kill()
                .map_err(|e| format!("Failed to kill PTY child: {}", e))?;
            info!("PTY session {} killed", run_id);
            Ok(())
        } else {
            Err(format!("No active PTY session: {}", run_id))
        }
    }

    /// Kill sessions owned by a closing window, excluding in-flight transfers.
    pub fn kill_window(&self, window_label: &str, protected: &HashSet<String>) -> Vec<String> {
        let removed: Vec<(String, LivePty)> = {
            let mut sessions = self.sessions.lock();
            let run_ids: Vec<String> = sessions
                .iter()
                .filter(|(run_id, live)| {
                    live.owner_window.as_deref() == Some(window_label)
                        && !protected.contains(*run_id)
                })
                .map(|(run_id, _)| run_id.clone())
                .collect();
            run_ids
                .into_iter()
                .filter_map(|run_id| sessions.remove(&run_id).map(|live| (run_id, live)))
                .collect()
        };
        let mut killed = Vec::with_capacity(removed.len());
        for (run_id, mut live) in removed {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if let Err(err) = live.killer.kill() {
                warn!(
                    "Failed to kill PTY child {} on window close: {}",
                    run_id, err
                );
            } else {
                info!("PTY session {} killed on window close", run_id);
            }
            killed.push(run_id);
        }
        killed
    }

    /// Kill all PTYs currently tracked by the service.
    pub fn kill_all(&self) -> usize {
        let live_sessions: Vec<(String, LivePty)> = self.sessions.lock().drain().collect();
        let total = live_sessions.len();
        for (run_id, mut live) in live_sessions {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if let Err(err) = live.killer.kill() {
                warn!(
                    "Failed to kill PTY child {} during shutdown: {}",
                    run_id, err
                );
            } else {
                info!("Cleanup: killed PTY {}", run_id);
            }
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::ipc::{Channel, InvokeResponseBody};

    #[derive(Debug, PartialEq)]
    enum Delivery {
        Output(Vec<u8>),
        Event(String),
    }

    fn channels(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyChannels {
        let output_deliveries = Arc::clone(&deliveries);
        let output = Channel::new(move |body| {
            let bytes = body.deserialize::<Vec<u8>>()?;
            output_deliveries.lock().push(Delivery::Output(bytes));
            Ok(())
        });
        let events = Channel::new(move |body| {
            let json = match body {
                InvokeResponseBody::Json(json) => json,
                InvokeResponseBody::Raw(_) => unreachable!("PTY events serialize as JSON"),
            };
            let value: serde_json::Value = serde_json::from_str(&json)?;
            deliveries.lock().push(Delivery::Event(
                value["type"].as_str().expect("event type").to_string(),
            ));
            Ok(())
        });
        PtyChannels { output, events }
    }

    fn sink(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyEventSink {
        let channels = channels(deliveries);
        PtyEventSink::new(channels.output, channels.events)
    }

    fn error_event() -> PtyEvent {
        PtyEvent::Error {
            run_id: "run".to_string(),
            message: "failed".to_string(),
        }
    }

    #[test]
    fn test_monotonic_sequence_order() {
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&deliveries));

        sink.emit_output(vec![1]);
        assert_eq!(sink.state.lock().last_dispatched, 1);
        sink.emit(error_event());
        assert_eq!(sink.state.lock().last_dispatched, 2);
        sink.emit_output(vec![2]);
        assert_eq!(sink.state.lock().last_dispatched, 3);
        assert_eq!(sink.sequence.load(Ordering::Acquire), 4);
        assert_eq!(
            *deliveries.lock(),
            vec![
                Delivery::Output(vec![1]),
                Delivery::Event("error".to_string()),
                Delivery::Output(vec![2]),
            ]
        );
    }

    #[test]
    fn test_pause_and_queue_while_detached() {
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(original);
        assert_eq!(sink.pause(), 0);

        sink.emit_output(vec![1, 2]);
        sink.emit_output(vec![3, 4]);
        assert_eq!(sink.state.lock().retained.len(), 2);

        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 2);
        assert_eq!(
            *attached.lock(),
            vec![Delivery::Output(vec![1, 2]), Delivery::Output(vec![3, 4]),]
        );
        sink.commit_transfer();
        assert!(sink.state.lock().retained.is_empty());
    }

    #[test]
    fn test_output_before_exit_draining() {
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&deliveries));

        sink.emit_output(b"tail".to_vec());
        assert!(!sink.complete(PtyEvent::Exit {
            run_id: "run".to_string(),
            code: Some(0),
        }));

        assert_eq!(
            *deliveries.lock(),
            vec![
                Delivery::Output(b"tail".to_vec()),
                Delivery::Event("exit".to_string()),
            ]
        );
        assert!(sink.is_completed());
    }

    #[test]
    fn test_resume_original() {
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&original));
        sink.pause();
        sink.emit_output(b"queued".to_vec());

        assert_eq!(sink.resume_original().unwrap(), 1);
        sink.emit_output(b"live".to_vec());

        assert_eq!(
            *original.lock(),
            vec![
                Delivery::Output(b"queued".to_vec()),
                Delivery::Output(b"live".to_vec()),
            ]
        );
    }

    #[test]
    fn test_completed_session_survives_attach_until_transfer_resolves() {
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&original));
        sink.pause();
        sink.emit_output(b"tail".to_vec());
        assert!(sink.complete(PtyEvent::Exit {
            run_id: "run".to_string(),
            code: Some(0),
        }));
        assert!(original.lock().is_empty());

        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 2);
        assert!(sink.is_completed());
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(b"tail".to_vec()),
                Delivery::Event("exit".to_string()),
            ]
        );

        sink.commit_transfer();
        assert!(sink.state.lock().original.is_none());
        assert!(original.lock().is_empty());
        assert_eq!(attached.lock().len(), 2);
    }

    #[test]
    fn test_detached_replay_preserves_output_event_order() {
        let sink = sink(Arc::new(Mutex::new(Vec::new())));
        sink.pause();
        sink.emit_output(vec![1]);
        sink.emit(error_event());
        sink.emit_output(vec![3]);
        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 3);
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(vec![1]),
                Delivery::Event("error".to_string()),
                Delivery::Output(vec![3]),
            ]
        );
    }

    #[test]
    fn test_abort_replays_target_deliveries_to_original() {
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&original));
        sink.pause();
        sink.emit_output(b"queued".to_vec());
        let attached = Arc::new(Mutex::new(Vec::new()));
        sink.attach(channels(Arc::clone(&attached))).unwrap();
        sink.emit_output(b"live".to_vec());
        sink.emit(error_event());
        assert!(original.lock().is_empty());
        assert_eq!(sink.resume_original().unwrap(), 3);
        assert_eq!(*original.lock(), *attached.lock());
        assert_eq!(
            *original.lock(),
            vec![
                Delivery::Output(b"queued".to_vec()),
                Delivery::Output(b"live".to_vec()),
                Delivery::Event("error".to_string()),
            ]
        );
    }

    #[test]
    fn test_retained_cap_blanks_output_without_dropping_sequences() {
        let sink = sink(Arc::new(Mutex::new(Vec::new())));
        sink.pause();
        sink.emit_output(vec![1; RETAINED_OUTPUT_CAP]);
        sink.emit(error_event());
        sink.emit_output(vec![3]);
        {
            let state = sink.state.lock();
            assert_eq!(state.retained_bytes, 1);
            assert_eq!(
                state
                    .retained
                    .iter()
                    .map(|entry| entry.sequence)
                    .collect::<Vec<_>>(),
                vec![1, 2, 3]
            );
        }
        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 3);
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(Vec::new()),
                Delivery::Event("error".to_string()),
                Delivery::Output(vec![3]),
            ]
        );
    }

    #[test]
    fn test_exit_after_target_attach_remains_available_for_rollback() {
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&original));
        sink.pause();
        let attached = Arc::new(Mutex::new(Vec::new()));
        sink.attach(channels(Arc::clone(&attached))).unwrap();
        assert!(sink.complete(PtyEvent::Exit {
            run_id: "run".to_string(),
            code: Some(0),
        }));
        sink.resume_original().unwrap();
        assert_eq!(*original.lock(), vec![Delivery::Event("exit".to_string())]);
        assert_eq!(*original.lock(), *attached.lock());
    }
}
