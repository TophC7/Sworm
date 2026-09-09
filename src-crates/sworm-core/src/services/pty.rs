use crate::events::EventSink;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use sworm_protocol::pty::PtyEvent;
use tracing::{error, info, warn};

const PTY_READ_BUF_SIZE: usize = 64 * 1024;
const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
const RETAINED_OUTPUT_CAP: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct PtyChannels {
    output: EventSink<Vec<u8>>,
    events: EventSink<PtyEvent>,
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
            Self::Output(bytes) => (channels.output)(bytes)
                .map_err(|_| "PTY output channel closed while attaching".to_string()),
            Self::Event(event) => (channels.events)(event)
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
        match payload {
            Payload::Output(bytes) => {
                self.retained_bytes += bytes.len();
                if let Some(last) = self.retained.last_mut() {
                    if last.delivered_to_target == delivered {
                        if let Payload::Output(retained) = &mut last.payload {
                            retained.extend(bytes);
                            last.sequence = sequence;
                        } else {
                            self.retained.push(Retained {
                                sequence,
                                payload: Payload::Output(bytes),
                                delivered_to_target: delivered,
                            });
                        }
                    } else {
                        self.retained.push(Retained {
                            sequence,
                            payload: Payload::Output(bytes),
                            delivered_to_target: delivered,
                        });
                    }
                } else {
                    self.retained.push(Retained {
                        sequence,
                        payload: Payload::Output(bytes),
                        delivered_to_target: delivered,
                    });
                }
            }
            Payload::Event(event) => self.retained.push(Retained {
                sequence,
                payload: Payload::Event(event),
                delivered_to_target: delivered,
            }),
        }
        if self.retained_bytes > RETAINED_OUTPUT_CAP {
            if !self.overflow_warned {
                warn!("PTY retained output exceeded cap; trimming oldest output bytes");
                self.overflow_warned = true;
            }
            for entry in &mut self.retained {
                let excess = self.retained_bytes - RETAINED_OUTPUT_CAP;
                if excess == 0 {
                    break;
                }
                if let Payload::Output(bytes) = &mut entry.payload {
                    let trim = excess.min(bytes.len());
                    bytes.drain(..trim);
                    self.retained_bytes -= trim;
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
/// attached subscriber and queues events while no subscriber is attached.
#[derive(Clone)]
pub struct PtyEventSink {
    sequence: Arc<AtomicU64>,
    run_id: String,
    state: Arc<Mutex<PtyStreamState>>,
}

impl PtyEventSink {
    fn new(run_id: String, output: EventSink<Vec<u8>>, events: EventSink<PtyEvent>) -> Self {
        Self {
            run_id,
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
        let SubscriberState::Active(channels) = &state.subscriber else {
            unreachable!("paused subscriber handled above");
        };
        let channels = channels.clone();
        if transferring {
            let delivered = payload.clone().send(&channels).is_ok();
            if delivered {
                state.last_dispatched = sequence;
            } else {
                state.subscriber = SubscriberState::Paused;
            }
            state.retain(sequence, payload, delivered);
        } else {
            match payload.send(&channels) {
                Ok(()) => state.last_dispatched = sequence,
                Err(err) => {
                    state.subscriber = SubscriberState::Paused;
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

    fn emit_synced(
        &self,
        state: &mut PtyStreamState,
        channels: &PtyChannels,
    ) -> Result<(), String> {
        (channels.events)(PtyEvent::Synced {
            run_id: self.run_id.clone(),
            sequence: self.sequence.load(Ordering::Acquire).saturating_sub(1),
        })
        .map_err(|_| {
            state.subscriber = SubscriberState::Paused;
            "PTY event channel closed while syncing".to_string()
        })
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
        self.emit_synced(&mut state, &channels)?;
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
        self.emit_synced(&mut state, &channels)?;
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
    owner_id: Option<String>,
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
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
        owner_id: Option<String>,
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
        let event_sink = PtyEventSink::new(run_id.clone(), output, events);

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
                owner_id,
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

    /// Atomically detach subscriber callbacks and return the last delivered sequence.
    pub fn pause(&self, run_id: &str) -> Result<u64, String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        Ok(sink.pause())
    }

    /// Attach replacement callbacks and deliver everything queued while detached.
    /// A completed session stays registered until the transfer commits or
    /// aborts so the broker can still resolve ownership.
    pub fn attach(
        &self,
        run_id: &str,
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
    ) -> Result<u64, String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        sink.attach(PtyChannels { output, events })
    }

    /// Abort a transfer and restore the callbacks detached by `pause`.
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

    /// Commit a transfer to its new owner, reaping the session if it
    /// already completed while detached.
    pub fn transfer_owner(&self, run_id: &str, new_owner_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock();
        let live = sessions
            .get_mut(run_id)
            .ok_or_else(|| format!("No PTY session: {}", run_id))?;
        live.owner_id = Some(new_owner_id.to_string());
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

    /// Kill sessions owned by a closing owner, excluding in-flight transfers.
    pub fn kill_owner(&self, owner_id: &str, protected: &HashSet<String>) -> Vec<String> {
        let removed: Vec<(String, LivePty)> = {
            let mut sessions = self.sessions.lock();
            let run_ids: Vec<String> = sessions
                .iter()
                .filter(|(run_id, live)| {
                    live.owner_id.as_deref() == Some(owner_id) && !protected.contains(*run_id)
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
                    "Failed to kill PTY child {} on owner close: {}",
                    run_id, err
                );
            } else {
                info!("PTY session {} killed on owner close", run_id);
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

    #[derive(Debug, PartialEq)]
    enum Delivery {
        Output(Vec<u8>),
        Event(&'static str),
    }

    fn event_kind(event: &PtyEvent) -> &'static str {
        match event {
            PtyEvent::Started { .. } => "started",
            PtyEvent::Exit { .. } => "exit",
            PtyEvent::Error { .. } => "error",
            PtyEvent::Synced { .. } => "synced",
            PtyEvent::ResumeTokenBound { .. } => "resumeTokenBound",
        }
    }

    fn channels(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyChannels {
        let output_deliveries = Arc::clone(&deliveries);
        let output = Arc::new(move |bytes| {
            output_deliveries.lock().push(Delivery::Output(bytes));
            Ok(())
        });
        let events = Arc::new(move |event: PtyEvent| {
            deliveries.lock().push(Delivery::Event(event_kind(&event)));
            Ok(())
        });
        PtyChannels { output, events }
    }

    fn channels_rejecting_events(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyChannels {
        let output = Arc::new(move |bytes| {
            deliveries.lock().push(Delivery::Output(bytes));
            Ok(())
        });
        let events = Arc::new(|_: PtyEvent| Err("closed".to_string()));
        PtyChannels { output, events }
    }

    fn sink(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyEventSink {
        let channels = channels(deliveries);
        PtyEventSink::new("run".to_string(), channels.output, channels.events)
    }

    fn error_event() -> PtyEvent {
        PtyEvent::Error {
            run_id: "run".to_string(),
            message: "failed".to_string(),
        }
    }

    #[test]
    fn test_sequence_barrier_matches_delivery_order() {
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&deliveries));

        sink.emit_output(vec![1]);
        sink.emit(error_event());
        sink.emit_output(vec![2]);

        assert_eq!(sink.pause(), 3);
        assert_eq!(
            *deliveries.lock(),
            vec![
                Delivery::Output(vec![1]),
                Delivery::Event("error"),
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

        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 2);
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(vec![1, 2, 3, 4]),
                Delivery::Event("synced"),
            ]
        );
        sink.commit_transfer();
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
            vec![Delivery::Output(b"tail".to_vec()), Delivery::Event("exit"),]
        );
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
                Delivery::Event("synced"),
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
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(b"tail".to_vec()),
                Delivery::Event("exit"),
                Delivery::Event("synced"),
            ]
        );

        sink.commit_transfer();
        assert!(original.lock().is_empty());
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
                Delivery::Event("error"),
                Delivery::Output(vec![3]),
                Delivery::Event("synced"),
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
        assert_eq!(
            *original.lock(),
            vec![
                Delivery::Output(b"queuedlive".to_vec()),
                Delivery::Event("error"),
                Delivery::Event("synced"),
            ]
        );
    }

    #[test]
    fn test_failed_attach_replays_full_order_to_original() {
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = sink(Arc::clone(&original));
        assert_eq!(sink.pause(), 0);
        sink.emit_output(b"A".to_vec());
        sink.emit(error_event());
        sink.emit_output(b"B".to_vec());

        let target = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(
            sink.attach(channels_rejecting_events(Arc::clone(&target))),
            Err("PTY event channel closed while attaching".to_string())
        );
        assert_eq!(*target.lock(), vec![Delivery::Output(b"A".to_vec())]);

        assert_eq!(sink.resume_original().unwrap(), 3);
        assert_eq!(
            *original.lock(),
            vec![
                Delivery::Output(b"A".to_vec()),
                Delivery::Event("error"),
                Delivery::Output(b"B".to_vec()),
                Delivery::Event("synced"),
            ]
        );
    }

    #[test]
    fn test_retained_cap_trims_oldest_output() {
        let sink = sink(Arc::new(Mutex::new(Vec::new())));
        sink.pause();
        sink.emit_output(vec![1; RETAINED_OUTPUT_CAP]);
        sink.emit(error_event());
        sink.emit_output(vec![3]);

        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 3);
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(vec![1; RETAINED_OUTPUT_CAP - 1]),
                Delivery::Event("error"),
                Delivery::Output(vec![3]),
                Delivery::Event("synced"),
            ]
        );
    }

    #[test]
    fn test_retained_output_coalesces_while_paused() {
        let sink = sink(Arc::new(Mutex::new(Vec::new())));
        sink.pause();
        sink.emit_output(vec![1]);
        sink.emit_output(vec![2]);
        sink.emit(error_event());
        sink.emit_output(vec![3]);
        sink.emit_output(vec![4]);

        let attached = Arc::new(Mutex::new(Vec::new()));
        assert_eq!(sink.attach(channels(Arc::clone(&attached))).unwrap(), 5);
        assert_eq!(
            *attached.lock(),
            vec![
                Delivery::Output(vec![1, 2]),
                Delivery::Event("error"),
                Delivery::Output(vec![3, 4]),
                Delivery::Event("synced"),
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
        assert_eq!(
            *original.lock(),
            vec![Delivery::Event("exit"), Delivery::Event("synced")]
        );
    }
}
