use crate::events::EventSink;
use crate::services::completed_runs::CompletedRun;
use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use sworm_protocol::pty::PtyEvent;
use sworm_protocol::rpc::PtyCursor;
use tracing::{error, info, warn};

const PTY_READ_BUF_SIZE: usize = 64 * 1024;
const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
const RETAINED_OUTPUT_CAP: usize = 8 * 1024 * 1024;
/// Coalescing ceiling for one retained output chunk. Trimming then drops whole
/// chunks off the front instead of memmoving the entire window every flush.
const RETAINED_CHUNK_CAP: usize = 256 * 1024;

/// Receives a window run's transcript once its process exited, so the run's
/// retained output can leave memory. Hosts without one keep the window.
pub type CompletedRunSink = Arc<dyn Fn(CompletedRun) + Send + Sync>;

#[derive(Clone)]
struct PtyChannels {
    output: EventSink<Vec<u8>>,
    events: EventSink<PtyEvent>,
}

/// An ordered PTY delivery with its wire replay cursor attached.
///
/// A single sink receives every variant so a caller can enqueue a gap,
/// replay, and subsequent live traffic without inferring cursors after send.
#[derive(Clone)]
pub enum PtyReplay {
    Gap { lost_bytes: u64 },
    Output { start_offset: u64, bytes: Vec<u8> },
    Event { sequence: u64, event: PtyEvent },
}

#[derive(Clone)]
enum Subscriber {
    Channels(PtyChannels),
    Replay {
        attachment: u64,
        sink: EventSink<PtyReplay>,
    },
}

enum SubscriberState {
    Active(Subscriber),
    Paused,
}

#[derive(Clone)]
enum Payload {
    Output(Vec<u8>),
    Event(PtyEvent),
}

impl Payload {
    fn send(
        self,
        subscriber: &Subscriber,
        start_offset: u64,
        event_sequence: u64,
    ) -> Result<(), String> {
        match (subscriber, self) {
            (Subscriber::Channels(channels), Self::Output(bytes)) => (channels.output)(bytes)
                .map_err(|_| "PTY output channel closed while attaching".to_string()),
            (Subscriber::Channels(channels), Self::Event(event)) => (channels.events)(event)
                .map_err(|_| "PTY event channel closed while attaching".to_string()),
            (Subscriber::Replay { sink, .. }, Self::Output(bytes)) => (sink)(PtyReplay::Output {
                start_offset,
                bytes,
            })
            .map_err(|_| "PTY replay channel closed while sending output".to_string()),
            (Subscriber::Replay { sink, .. }, Self::Event(event)) => (sink)(PtyReplay::Event {
                sequence: event_sequence,
                event,
            })
            .map_err(|_| "PTY replay channel closed while sending event".to_string()),
        }
    }
}

struct Retained {
    sequence: u64,
    start_offset: u64,
    event_sequence: u64,
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
    output_offset: u64,
    event_sequence: u64,
    next_attachment: u64,
    window: bool,
    completed: bool,
    exit_code: Option<Option<i32>>,
}

impl PtyStreamState {
    fn retain(
        &mut self,
        sequence: u64,
        start_offset: u64,
        event_sequence: u64,
        payload: Payload,
        delivered: bool,
    ) {
        match payload {
            Payload::Output(bytes) => {
                self.retained_bytes += bytes.len();
                let coalesce = matches!(
                    self.retained.last(),
                    Some(Retained {
                        payload: Payload::Output(retained),
                        delivered_to_target,
                        ..
                    }) if *delivered_to_target == delivered
                        && retained.len() + bytes.len() <= RETAINED_CHUNK_CAP
                );
                match self.retained.last_mut() {
                    Some(Retained {
                        payload: Payload::Output(retained),
                        sequence: last_sequence,
                        start_offset: last_offset,
                        ..
                    }) if coalesce => {
                        debug_assert_eq!(*last_offset + retained.len() as u64, start_offset);
                        retained.extend(bytes);
                        *last_sequence = sequence;
                    }
                    _ => self.retained.push(Retained {
                        sequence,
                        start_offset,
                        event_sequence: 0,
                        payload: Payload::Output(bytes),
                        delivered_to_target: delivered,
                    }),
                }
            }
            Payload::Event(event) => self.retained.push(Retained {
                sequence,
                start_offset: 0,
                event_sequence,
                payload: Payload::Event(event),
                delivered_to_target: delivered,
            }),
        }
        if self.retained_bytes > RETAINED_OUTPUT_CAP {
            if !self.overflow_warned {
                warn!("PTY retained output exceeded cap; trimming oldest output bytes");
                self.overflow_warned = true;
            }
            let mut excess = self.retained_bytes - RETAINED_OUTPUT_CAP;
            for entry in &mut self.retained {
                if excess == 0 {
                    break;
                }
                let Payload::Output(bytes) = &mut entry.payload else {
                    continue;
                };
                let trim = excess.min(bytes.len());
                if trim == bytes.len() {
                    bytes.clear();
                } else {
                    bytes.drain(..trim);
                }
                entry.start_offset += trim as u64;
                self.retained_bytes -= trim;
                excess -= trim;
            }
            self.retained.retain(
                |entry| !matches!(&entry.payload, Payload::Output(bytes) if bytes.is_empty()),
            );
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
    fn new(
        run_id: String,
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
        window: bool,
    ) -> Self {
        Self {
            run_id,
            sequence: Arc::new(AtomicU64::new(1)),
            state: Arc::new(Mutex::new(PtyStreamState {
                subscriber: SubscriberState::Active(Subscriber::Channels(PtyChannels {
                    output,
                    events,
                })),
                original: None,
                retained: Vec::new(),
                retained_bytes: 0,
                overflow_warned: false,
                last_dispatched: 0,
                output_offset: 0,
                event_sequence: 0,
                next_attachment: 0,
                window,
                completed: false,
                exit_code: None,
            })),
        }
    }

    fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    pub fn emit_output(&self, bytes: Vec<u8>) {
        self.emit_payload(Payload::Output(bytes));
    }

    pub fn emit(&self, event: PtyEvent) {
        self.emit_payload(Payload::Event(event));
    }

    /// Tell the current subscriber that everything emitted so far is delivered.
    ///
    /// An adopted run forwards its backend's replay boundary this way: the
    /// remote sequence number means nothing locally, so the marker is restamped
    /// with this sink's own sequence instead of being relayed verbatim.
    pub fn emit_sync_marker(&self) {
        let mut state = self.state.lock();
        let SubscriberState::Active(subscriber) = &state.subscriber else {
            return;
        };
        let subscriber = subscriber.clone();
        let _ = self.emit_synced(&mut state, &subscriber);
    }

    fn emit_payload(&self, payload: Payload) {
        let mut state = self.state.lock();
        self.emit_payload_locked(&mut state, payload);
    }

    fn emit_payload_locked(&self, state: &mut PtyStreamState, payload: Payload) {
        let sequence = self.next_sequence();
        let (start_offset, event_sequence) = match &payload {
            Payload::Output(bytes) => {
                let start_offset = state.output_offset;
                state.output_offset = state
                    .output_offset
                    .checked_add(bytes.len() as u64)
                    .expect("PTY output offset overflow");
                (start_offset, 0)
            }
            Payload::Event(_) => {
                state.event_sequence = state
                    .event_sequence
                    .checked_add(1)
                    .expect("PTY event sequence overflow");
                (0, state.event_sequence)
            }
        };

        if matches!(&state.subscriber, SubscriberState::Paused) {
            state.retain(sequence, start_offset, event_sequence, payload, false);
            return;
        }
        let transferring = state.original.is_some();
        let SubscriberState::Active(subscriber) = &state.subscriber else {
            unreachable!("paused subscriber handled above");
        };
        let subscriber = subscriber.clone();
        let delivered = payload
            .clone()
            .send(&subscriber, start_offset, event_sequence)
            .is_ok();
        if delivered {
            state.last_dispatched = sequence;
        } else {
            state.subscriber = SubscriberState::Paused;
            warn!("PTY subscriber channel closed");
        }
        if transferring || state.window {
            state.retain(sequence, start_offset, event_sequence, payload, delivered);
        }
    }

    fn complete(&self, event: PtyEvent) -> bool {
        let exit_code = match &event {
            PtyEvent::Exit { code, .. } => Some(*code),
            _ => None,
        };
        let mut state = self.state.lock();
        state.completed = true;
        state.exit_code = exit_code;
        self.emit_payload_locked(&mut state, Payload::Event(event));
        state.window
            || state.original.is_some()
            || matches!(&state.subscriber, SubscriberState::Paused)
    }

    fn pause(&self) -> u64 {
        let mut state = self.state.lock();
        let channels = match &state.subscriber {
            SubscriberState::Active(Subscriber::Channels(channels)) => Some(channels.clone()),
            SubscriberState::Active(Subscriber::Replay { .. }) | SubscriberState::Paused => None,
        };
        if !state.window {
            if let Some(channels) = channels {
                if state.original.is_none() {
                    state.original = Some(channels);
                }
            }
        }
        state.subscriber = SubscriberState::Paused;
        state.last_dispatched
    }

    fn pause_attachment(&self, attachment: u64) -> bool {
        let mut state = self.state.lock();
        let current = matches!(
            &state.subscriber,
            SubscriberState::Active(Subscriber::Replay {
                attachment: current,
                ..
            }) if *current == attachment
        );
        if current {
            state.subscriber = SubscriberState::Paused;
        }
        current
    }

    fn synced_event(&self) -> PtyEvent {
        PtyEvent::Synced {
            run_id: self.run_id.clone(),
            sequence: self.sequence.load(Ordering::Acquire).saturating_sub(1),
        }
    }

    fn emit_synced(
        &self,
        state: &mut PtyStreamState,
        subscriber: &Subscriber,
    ) -> Result<(), String> {
        let event = self.synced_event();
        let result = match subscriber {
            Subscriber::Channels(channels) => (channels.events)(event),
            Subscriber::Replay { sink, .. } => (sink)(PtyReplay::Event {
                sequence: state.event_sequence,
                event,
            }),
        };
        result.map_err(|_| {
            state.subscriber = SubscriberState::Paused;
            "PTY event channel closed while syncing".to_string()
        })
    }

    fn attach(&self, channels: PtyChannels) -> Result<u64, String> {
        let mut state = self.state.lock();
        if state.window {
            return Err("Window-mode PTY requires cursor attachment".to_string());
        }
        let subscriber = Subscriber::Channels(channels);
        state.subscriber = SubscriberState::Active(subscriber.clone());

        for index in 0..state.retained.len() {
            if state.retained[index].delivered_to_target {
                continue;
            }
            let entry = &state.retained[index];
            if let Err(err) =
                entry
                    .payload
                    .clone()
                    .send(&subscriber, entry.start_offset, entry.event_sequence)
            {
                state.subscriber = SubscriberState::Paused;
                return Err(err);
            }
            state.retained[index].delivered_to_target = true;
            state.last_dispatched = state.retained[index].sequence;
        }
        self.emit_synced(&mut state, &subscriber)?;
        if state.original.is_none() {
            state.clear_retained();
        }

        Ok(self.sequence.load(Ordering::Acquire).saturating_sub(1))
    }

    fn attach_from(&self, sink: EventSink<PtyReplay>, cursor: PtyCursor) -> Result<u64, String> {
        let mut state = self.state.lock();
        if !state.window {
            return Err("Cursor attachment requires a window-mode PTY".to_string());
        }
        state.next_attachment = state
            .next_attachment
            .checked_add(1)
            .expect("PTY attachment sequence overflow");
        let attachment = state.next_attachment;
        let subscriber = Subscriber::Replay {
            attachment,
            sink: sink.clone(),
        };
        state.subscriber = SubscriberState::Active(subscriber.clone());

        if let Some(first_offset) = state.retained.iter().find_map(|entry| {
            matches!(&entry.payload, Payload::Output(bytes) if !bytes.is_empty())
                .then_some(entry.start_offset)
        }) {
            if cursor.output_offset < first_offset {
                sink(PtyReplay::Gap {
                    lost_bytes: first_offset - cursor.output_offset,
                })
                .map_err(|_| {
                    state.subscriber = SubscriberState::Paused;
                    "PTY replay channel closed while sending gap".to_string()
                })?;
            }
        }

        for index in 0..state.retained.len() {
            let entry = &state.retained[index];
            let entry_sequence = entry.sequence;
            let delivery = match &entry.payload {
                Payload::Output(bytes) => {
                    let skip = cursor.output_offset.saturating_sub(entry.start_offset);
                    let skip = usize::try_from(skip).unwrap_or(usize::MAX).min(bytes.len());
                    (skip < bytes.len()).then(|| PtyReplay::Output {
                        start_offset: entry.start_offset + skip as u64,
                        bytes: bytes[skip..].to_vec(),
                    })
                }
                Payload::Event(event) if entry.event_sequence > cursor.event_sequence => {
                    Some(PtyReplay::Event {
                        sequence: entry.event_sequence,
                        event: event.clone(),
                    })
                }
                Payload::Event(_) => None,
            };
            if let Some(delivery) = delivery {
                if sink(delivery).is_err() {
                    state.subscriber = SubscriberState::Paused;
                    return Err("PTY replay channel closed while attaching".to_string());
                }
                state.last_dispatched = entry_sequence;
            }
        }
        self.emit_synced(&mut state, &subscriber)?;
        Ok(attachment)
    }

    fn resume_original(&self) -> Result<u64, String> {
        let mut state = self.state.lock();
        let channels = state
            .original
            .take()
            .ok_or_else(|| "PTY has no original subscriber to restore".to_string())?;
        let subscriber = Subscriber::Channels(channels);
        state.subscriber = SubscriberState::Active(subscriber.clone());
        // Target delivery flags do not apply to the restored original subscriber.
        for entry in &mut state.retained {
            entry.delivered_to_target = false;
        }
        for index in 0..state.retained.len() {
            let entry = &state.retained[index];
            if let Err(err) =
                entry
                    .payload
                    .clone()
                    .send(&subscriber, entry.start_offset, entry.event_sequence)
            {
                state.subscriber = SubscriberState::Paused;
                return Err(err);
            }
            state.retained[index].delivered_to_target = true;
            state.last_dispatched = state.retained[index].sequence;
        }
        self.emit_synced(&mut state, &subscriber)?;
        state.clear_retained();
        Ok(self.sequence.load(Ordering::Acquire).saturating_sub(1))
    }

    fn commit_transfer(&self) {
        let mut state = self.state.lock();
        state.original = None;
        if !state.window {
            state.clear_retained();
        }
    }

    fn is_completed(&self) -> bool {
        self.state.lock().completed
    }

    fn exit_code(&self) -> Option<Option<i32>> {
        self.state.lock().exit_code
    }

    /// Drain the retained window into a transcript for durable storage.
    fn take_completed(&self) -> CompletedRun {
        let mut state = self.state.lock();
        let mut output_start = None;
        let mut output = Vec::with_capacity(state.retained_bytes);
        let mut events = Vec::new();
        for entry in state.retained.drain(..) {
            match entry.payload {
                Payload::Output(bytes) => {
                    output_start.get_or_insert(entry.start_offset);
                    output.extend(bytes);
                }
                Payload::Event(event) => events.push((entry.event_sequence, event)),
            }
        }
        state.retained_bytes = 0;
        state.overflow_warned = false;
        CompletedRun {
            run_id: self.run_id.clone(),
            exit_code: state.exit_code.flatten(),
            // An empty window still starts where the run's output ended.
            output_start: output_start.unwrap_or(state.output_offset),
            output,
            events,
        }
    }
}

/// Process control behind a tracked run.
pub trait RunBackend: Send + Sync {
    fn write(&self, data: &[u8]) -> Result<(), String>;
    fn resize(&self, cols: u16, rows: u16) -> Result<(), String>;
    fn kill(&self) -> Result<(), String>;

    /// Wire-backed runs detach when the application shuts down.
    fn detaches_on_shutdown(&self) -> bool;
}

struct LocalBackend {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
}

impl RunBackend for LocalBackend {
    fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut writer = self.writer.lock();
        writer
            .write_all(data)
            .map_err(|error| format!("PTY write failed: {error}"))?;
        writer
            .flush()
            .map_err(|error| format!("PTY flush failed: {error}"))
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .lock()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("PTY resize failed: {error}"))
    }

    fn kill(&self) -> Result<(), String> {
        self.killer
            .lock()
            .kill()
            .map_err(|error| format!("Failed to kill PTY child: {error}"))
    }

    fn detaches_on_shutdown(&self) -> bool {
        false
    }
}

/// State of a known run. Absence means the run is unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtyRunState {
    Live,
    Completed(Option<i32>),
}

/// A single live run or a completed window whose tail remains replayable.
struct LivePty {
    backend: Box<dyn RunBackend>,
    shutdown: Arc<AtomicBool>,
    finalized: Arc<AtomicBool>,
    runtime_id: String,
    event_sink: PtyEventSink,
    owner_id: Option<String>,
}

/// PTY service managing active sessions and completed retained windows.
pub struct PtyService {
    sessions: Arc<Mutex<HashMap<String, LivePty>>>,
    completed: Mutex<Option<CompletedRunSink>>,
}

impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            completed: Mutex::new(None),
        }
    }

    /// Persist finished window runs instead of keeping their output in
    /// memory. A host that outlives its clients (the daemon) sets this.
    pub fn on_completed_run(&self, sink: CompletedRunSink) {
        *self.completed.lock() = Some(sink);
    }

    /// Spawn a new local PTY. An existing run with the same id is killed first.
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
        window: bool,
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
            .map_err(|error| format!("Failed to open PTY: {error}"))?;

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
            .map_err(|error| format!("Failed to spawn process: {error}"))?;
        let pid = child.process_id();
        let killer = child.clone_killer();
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("Failed to get PTY writer: {error}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("Failed to clone PTY reader: {error}"))?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));
        let runtime_id = uuid::Uuid::new_v4().to_string();
        let event_sink = PtyEventSink::new(run_id.clone(), output, events, window);
        let archive = window.then(|| self.completed.lock().clone()).flatten();

        self.sessions.lock().insert(
            run_id.clone(),
            LivePty {
                backend: Box::new(LocalBackend {
                    master: Mutex::new(pair.master),
                    writer: Mutex::new(writer),
                    killer: Mutex::new(killer),
                }),
                shutdown: Arc::clone(&shutdown),
                finalized: Arc::clone(&finalized),
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
            .name(format!("pty-reader-{run_id}"))
            .spawn(move || {
                let mut buf = [0u8; PTY_READ_BUF_SIZE];
                let pending: Arc<Mutex<Vec<u8>>> =
                    Arc::new(Mutex::new(Vec::with_capacity(64 * 1024)));
                let flusher_stop = Arc::new(AtomicBool::new(false));

                let pending_for_flusher = Arc::clone(&pending);
                let flusher_stop_inner = Arc::clone(&flusher_stop);
                let sink_for_flusher = sink_for_thread.clone();
                let flusher_handle = std::thread::Builder::new()
                    .name(format!("pty-flusher-{sid_for_thread}"))
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
                    .unwrap_or_else(|error| {
                        error!("Failed to spawn PTY flusher thread: {error}");
                        None
                    });

                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        info!("PTY shutdown requested for {sid_for_thread}");
                        break;
                    }
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            info!("PTY reader EOF for {sid_for_thread}");
                            break;
                        }
                        Ok(n) => pending.lock().extend_from_slice(&buf[..n]),
                        Err(error) => {
                            if shutdown.load(Ordering::Relaxed) {
                                info!("PTY read loop stopped during shutdown for {sid_for_thread}");
                            } else {
                                error!("PTY read error for {sid_for_thread}: {error}");
                                sink_for_thread.emit(PtyEvent::Error {
                                    run_id: sid_for_thread.clone(),
                                    message: error.to_string(),
                                });
                            }
                            break;
                        }
                    }
                }

                // Flush the output tail before assigning the Exit event sequence.
                flusher_stop.store(true, Ordering::Release);
                if let Some(handle) = flusher_handle {
                    if let Err(error) = handle.join() {
                        warn!("PTY flusher thread panicked: {error:?}");
                    }
                }

                let exit_status = child.wait();
                let exit_code = exit_status
                    .as_ref()
                    .ok()
                    .map(|status| status.exit_code() as i32);
                if let Err(error) = exit_status {
                    warn!("Failed waiting for PTY child {sid_for_thread}: {error}");
                }

                if finalized.swap(true, Ordering::AcqRel) {
                    info!("Skipping duplicate PTY finalization for {sid_for_thread}");
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
                    let Some(archive) = archive else {
                        info!("PTY {sid_for_thread} exited; retaining queued tail");
                        return;
                    };
                    // The transcript outlives this process on disk, so the
                    // window can leave memory with the run.
                    archive(sink_for_thread.take_completed());
                    info!("PTY {sid_for_thread} exited; transcript stored");
                }

                let mut sessions = sessions_for_thread.lock();
                let remove_current = sessions
                    .get(&sid_for_thread)
                    .is_some_and(|live| live.runtime_id == runtime_id);
                if remove_current {
                    sessions.remove(&sid_for_thread);
                }
            })
            .map_err(|error| format!("Failed to spawn reader thread: {error}"))?;

        Ok(event_sink)
    }

    /// Adopt a non-local run. Duplicate ids are rejected instead of replacing
    /// the existing run because replacement could orphan its remote process.
    pub fn adopt(
        &self,
        run_id: String,
        backend: Box<dyn RunBackend>,
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
        owner_id: Option<String>,
    ) -> Result<PtyEventSink, String> {
        let mut sessions = self.sessions.lock();
        if sessions.contains_key(&run_id) {
            return Err(format!("PTY session already exists: {run_id}"));
        }
        let event_sink = PtyEventSink::new(run_id.clone(), output, events, false);
        sessions.insert(
            run_id,
            LivePty {
                backend,
                shutdown: Arc::new(AtomicBool::new(false)),
                finalized: Arc::new(AtomicBool::new(false)),
                runtime_id: uuid::Uuid::new_v4().to_string(),
                event_sink: event_sink.clone(),
                owner_id,
            },
        );
        Ok(event_sink)
    }

    /// Atomically detach subscriber callbacks and return the last delivered sequence.
    pub fn pause(&self, run_id: &str) -> Result<u64, String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {run_id}"))?;
        Ok(sink.pause())
    }

    /// Pause only if `attachment` still owns the window subscriber. A stale
    /// stream cannot detach a newer replacement stream during cleanup.
    pub fn pause_attachment(&self, run_id: &str, attachment: u64) -> Result<bool, String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {run_id}"))?;
        Ok(sink.pause_attachment(attachment))
    }

    /// Attach replacement callbacks for a non-window transfer.
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
            .ok_or_else(|| format!("No PTY session: {run_id}"))?;
        sink.attach(PtyChannels { output, events })
    }

    /// Attach a cursor-aware subscriber to a retained window.
    ///
    /// Gap, replay, Synced, and live deliveries all use one ordered sink while
    /// stream state is locked. The sink may therefore update its cursor from
    /// each delivery without sampling mutable PTY state after the send.
    pub fn attach_from(
        &self,
        run_id: &str,
        sink: EventSink<PtyReplay>,
        cursor: PtyCursor,
    ) -> Result<u64, String> {
        let event_sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {run_id}"))?;
        event_sink.attach_from(sink, cursor)
    }

    /// Cursor immediately after all output and events emitted by a known run.
    pub fn cursor(&self, run_id: &str) -> Option<PtyCursor> {
        self.sessions.lock().get(run_id).map(|live| {
            let state = live.event_sink.state.lock();
            PtyCursor {
                output_offset: state.output_offset,
                event_sequence: state.event_sequence,
            }
        })
    }

    /// Distinguish live, completed-with-replay, and unknown runs.
    pub fn run_state(&self, run_id: &str) -> Option<PtyRunState> {
        self.sessions.lock().get(run_id).map(|live| {
            live.event_sink
                .exit_code()
                .map_or(PtyRunState::Live, PtyRunState::Completed)
        })
    }

    /// Complete an adopted run after its backend reports Exit.
    pub fn complete_adopted(&self, run_id: &str, code: Option<i32>) -> Result<(), String> {
        let (event_sink, finalized, runtime_id) = {
            let sessions = self.sessions.lock();
            let live = sessions
                .get(run_id)
                .ok_or_else(|| format!("No PTY session: {run_id}"))?;
            if !live.backend.detaches_on_shutdown() {
                return Err(format!("PTY session is not adopted: {run_id}"));
            }
            (
                live.event_sink.clone(),
                Arc::clone(&live.finalized),
                live.runtime_id.clone(),
            )
        };
        if finalized.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let retain = event_sink.complete(PtyEvent::Exit {
            run_id: run_id.to_string(),
            code,
        });
        if !retain {
            let mut sessions = self.sessions.lock();
            if sessions
                .get(run_id)
                .is_some_and(|live| live.runtime_id == runtime_id)
            {
                sessions.remove(run_id);
            }
        }
        Ok(())
    }

    /// Abort a transfer and restore the callbacks detached by `pause`.
    pub fn resume_original(&self, run_id: &str) -> Result<(), String> {
        let sink = self
            .sessions
            .lock()
            .get(run_id)
            .map(|live| live.event_sink.clone())
            .ok_or_else(|| format!("No PTY session: {run_id}"))?;
        sink.resume_original()?;
        if sink.is_completed() {
            self.sessions.lock().remove(run_id);
        }
        Ok(())
    }

    /// Commit a transfer to its new owner, reaping a completed non-window run.
    pub fn transfer_owner(&self, run_id: &str, new_owner_id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock();
        let live = sessions
            .get_mut(run_id)
            .ok_or_else(|| format!("No PTY session: {run_id}"))?;
        live.owner_id = Some(new_owner_id.to_string());
        live.event_sink.commit_transfer();
        if live.event_sink.is_completed() {
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

    /// Write data to a live run.
    pub fn write(&self, run_id: &str, data: &[u8]) -> Result<(), String> {
        let sessions = self.sessions.lock();
        let live = sessions
            .get(run_id)
            .filter(|live| !live.event_sink.is_completed())
            .ok_or_else(|| format!("No active PTY session: {run_id}"))?;
        live.backend.write(data)
    }

    /// Resize a live run.
    pub fn resize(&self, run_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock();
        let live = sessions
            .get(run_id)
            .filter(|live| !live.event_sink.is_completed())
            .ok_or_else(|| format!("No active PTY session: {run_id}"))?;
        live.backend.resize(cols, rows)
    }

    /// Kill a run explicitly. This always kills an adopted backend.
    pub fn kill(&self, run_id: &str) -> Result<(), String> {
        let live = self
            .sessions
            .lock()
            .remove(run_id)
            .ok_or_else(|| format!("No active PTY session: {run_id}"))?;
        live.shutdown.store(true, Ordering::Relaxed);
        live.finalized.store(true, Ordering::Release);
        if !live.event_sink.is_completed() {
            live.backend.kill()?;
        }
        info!("PTY session {run_id} killed");
        Ok(())
    }

    fn take_owner_runs(
        &self,
        owner_id: &str,
        protected: &HashSet<String>,
    ) -> Vec<(String, LivePty)> {
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
    }

    /// Kill every run owned by a closing non-last owner.
    pub fn kill_owner(&self, owner_id: &str, protected: &HashSet<String>) -> Vec<String> {
        let removed = self.take_owner_runs(owner_id, protected);
        let mut killed = Vec::with_capacity(removed.len());
        for (run_id, live) in removed {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if let Err(error) = live.backend.kill() {
                warn!("Failed to kill PTY {run_id} on owner close: {error}");
            } else {
                info!("PTY session {run_id} killed on owner close");
            }
            killed.push(run_id);
        }
        killed
    }

    /// Release last-owner runs, detaching adopted backends but killing locals.
    pub fn detach_owner(&self, owner_id: &str, protected: &HashSet<String>) -> Vec<String> {
        let removed = self.take_owner_runs(owner_id, protected);
        let mut released = Vec::with_capacity(removed.len());
        for (run_id, live) in removed {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if live.backend.detaches_on_shutdown() {
                info!("PTY session {run_id} detached on owner close");
            } else if let Err(error) = live.backend.kill() {
                warn!("Failed to kill local PTY {run_id} on owner close: {error}");
            } else {
                info!("PTY session {run_id} killed on owner close");
            }
            released.push(run_id);
        }
        released
    }

    /// Drop every shutdown-detaching backend and leave local runs tracked.
    pub fn detach_all(&self) -> usize {
        let detached = {
            let mut sessions = self.sessions.lock();
            let run_ids: Vec<String> = sessions
                .iter()
                .filter(|(_, live)| live.backend.detaches_on_shutdown())
                .map(|(run_id, _)| run_id.clone())
                .collect();
            run_ids
                .into_iter()
                .filter_map(|run_id| sessions.remove(&run_id))
                .collect::<Vec<_>>()
        };
        let count = detached.len();
        for live in detached {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
        }
        count
    }

    /// Shut down locals and detach adopted runs. Returns the number killed;
    /// shutdown-detaching runs are intentionally not counted as killed.
    pub fn kill_all(&self) -> usize {
        self.detach_all();
        let local_runs: Vec<(String, LivePty)> = self.sessions.lock().drain().collect();
        let killed = local_runs.len();
        for (run_id, live) in local_runs {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if let Err(error) = live.backend.kill() {
                warn!("Failed to kill PTY {run_id} during shutdown: {error}");
            } else {
                info!("Cleanup: killed PTY {run_id}");
            }
        }
        killed
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
        PtyEventSink::new("run".to_string(), channels.output, channels.events, false)
    }

    fn window_sink(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyEventSink {
        let channels = channels(deliveries);
        PtyEventSink::new("run".to_string(), channels.output, channels.events, true)
    }

    #[derive(Debug, PartialEq)]
    enum ReplayDelivery {
        Gap(u64),
        Output(u64, Vec<u8>),
        Event(u64, &'static str),
    }

    fn replay_sink(deliveries: Arc<Mutex<Vec<ReplayDelivery>>>) -> EventSink<PtyReplay> {
        Arc::new(move |delivery| {
            deliveries.lock().push(match delivery {
                PtyReplay::Gap { lost_bytes } => ReplayDelivery::Gap(lost_bytes),
                PtyReplay::Output {
                    start_offset,
                    bytes,
                } => ReplayDelivery::Output(start_offset, bytes),
                PtyReplay::Event { sequence, event } => {
                    ReplayDelivery::Event(sequence, event_kind(&event))
                }
            });
            Ok(())
        })
    }

    fn noop_channels() -> (EventSink<Vec<u8>>, EventSink<PtyEvent>) {
        (Arc::new(|_| Ok(())), Arc::new(|_| Ok(())))
    }

    struct FakeBackend {
        killed: Arc<AtomicBool>,
        detaches: bool,
    }

    impl RunBackend for FakeBackend {
        fn write(&self, _data: &[u8]) -> Result<(), String> {
            Ok(())
        }

        fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
            Ok(())
        }

        fn kill(&self) -> Result<(), String> {
            self.killed.store(true, Ordering::Release);
            Ok(())
        }

        fn detaches_on_shutdown(&self) -> bool {
            self.detaches
        }
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

    #[test]
    fn window_mode_retains_while_active() {
        let active = Arc::new(Mutex::new(Vec::new()));
        let sink = window_sink(active);
        sink.emit_output(b"A".to_vec());
        sink.emit_output(b"B".to_vec());

        let replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(
            replay_sink(Arc::clone(&replay)),
            PtyCursor {
                output_offset: 1,
                event_sequence: 0,
            },
        )
        .unwrap();

        assert_eq!(
            *replay.lock(),
            vec![
                ReplayDelivery::Output(1, b"B".to_vec()),
                ReplayDelivery::Event(0, "synced"),
            ]
        );
    }

    #[test]
    fn window_mode_cursor_inside_coalesced_chunk() {
        let sink = window_sink(Arc::new(Mutex::new(Vec::new())));
        sink.emit_output(vec![1, 2, 3]);
        sink.emit_output(vec![4, 5]);

        let replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(
            replay_sink(Arc::clone(&replay)),
            PtyCursor {
                output_offset: 4,
                event_sequence: 0,
            },
        )
        .unwrap();

        assert_eq!(
            *replay.lock(),
            vec![
                ReplayDelivery::Output(4, vec![5]),
                ReplayDelivery::Event(0, "synced"),
            ]
        );
    }

    #[test]
    fn window_mode_events_replay_by_sequence() {
        let sink = window_sink(Arc::new(Mutex::new(Vec::new())));
        sink.emit_output(b"A".to_vec());
        sink.emit(error_event());
        sink.emit_output(b"B".to_vec());

        let output_replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(
            replay_sink(Arc::clone(&output_replay)),
            PtyCursor {
                output_offset: 1,
                event_sequence: 1,
            },
        )
        .unwrap();
        assert_eq!(
            *output_replay.lock(),
            vec![
                ReplayDelivery::Output(1, b"B".to_vec()),
                ReplayDelivery::Event(1, "synced"),
            ]
        );

        let event_replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(
            replay_sink(Arc::clone(&event_replay)),
            PtyCursor {
                output_offset: 2,
                event_sequence: 0,
            },
        )
        .unwrap();
        assert_eq!(
            *event_replay.lock(),
            vec![
                ReplayDelivery::Event(1, "error"),
                ReplayDelivery::Event(1, "synced"),
            ]
        );
    }

    #[test]
    fn window_mode_gap_when_trimmed_past_cursor() {
        let sink = window_sink(Arc::new(Mutex::new(Vec::new())));
        sink.emit_output(vec![7; RETAINED_OUTPUT_CAP + 4]);

        let replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(replay_sink(Arc::clone(&replay)), PtyCursor::default())
            .unwrap();
        let deliveries = replay.lock();
        assert_eq!(deliveries.first(), Some(&ReplayDelivery::Gap(4)));
        assert_eq!(
            deliveries.get(1),
            Some(&ReplayDelivery::Output(4, vec![7; RETAINED_OUTPUT_CAP]))
        );
        assert_eq!(deliveries.get(2), Some(&ReplayDelivery::Event(0, "synced")));
    }

    #[test]
    fn completed_window_replays_tail_before_exit() {
        let sink = window_sink(Arc::new(Mutex::new(Vec::new())));
        sink.emit_output(b"tail".to_vec());
        assert!(sink.complete(PtyEvent::Exit {
            run_id: "run".to_string(),
            code: Some(0),
        }));

        let replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(replay_sink(Arc::clone(&replay)), PtyCursor::default())
            .unwrap();
        assert_eq!(
            *replay.lock(),
            vec![
                ReplayDelivery::Output(0, b"tail".to_vec()),
                ReplayDelivery::Event(1, "exit"),
                ReplayDelivery::Event(1, "synced"),
            ]
        );
    }

    #[test]
    fn adopted_run_kill_all_detaches() {
        let service = PtyService::new();
        let cwd = std::env::temp_dir();
        let adopted_killed = Arc::new(AtomicBool::new(false));
        let (output, events) = noop_channels();
        service
            .adopt(
                "adopted".to_string(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&adopted_killed),
                    detaches: true,
                }),
                output,
                events,
                None,
            )
            .unwrap();

        let (output, events) = noop_channels();
        service
            .spawn(
                "local".to_string(),
                "sh",
                &["-c", "sleep 60"],
                Some(cwd.to_str().unwrap()),
                None,
                80,
                24,
                output,
                events,
                None,
                false,
                None,
            )
            .unwrap();

        assert_eq!(service.kill_all(), 1);
        assert!(!adopted_killed.load(Ordering::Acquire));
    }

    #[test]
    fn adopted_run_kill_owner_kills() {
        let service = PtyService::new();
        let killed = Arc::new(AtomicBool::new(false));
        let (output, events) = noop_channels();
        service
            .adopt(
                "adopted".to_string(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&killed),
                    detaches: true,
                }),
                output,
                events,
                Some("owner".to_string()),
            )
            .unwrap();

        assert_eq!(
            service.kill_owner("owner", &HashSet::new()),
            vec!["adopted".to_string()]
        );
        assert!(killed.load(Ordering::Acquire));
    }

    #[test]
    fn detach_owner_detaches_adopted_and_kills_local() {
        let service = PtyService::new();
        let adopted_killed = Arc::new(AtomicBool::new(false));
        let local_killed = Arc::new(AtomicBool::new(false));
        for (run_id, killed, detaches) in [
            ("adopted", Arc::clone(&adopted_killed), true),
            ("local", Arc::clone(&local_killed), false),
        ] {
            let (output, events) = noop_channels();
            service
                .adopt(
                    run_id.to_string(),
                    Box::new(FakeBackend { killed, detaches }),
                    output,
                    events,
                    Some("owner".to_string()),
                )
                .unwrap();
        }

        let mut released = service.detach_owner("owner", &HashSet::new());
        released.sort();
        assert_eq!(released, vec!["adopted".to_string(), "local".to_string()]);
        assert!(!adopted_killed.load(Ordering::Acquire));
        assert!(local_killed.load(Ordering::Acquire));
        assert_eq!(service.run_state("adopted"), None);
        assert_eq!(service.run_state("local"), None);
    }

    #[test]
    fn stale_attachment_cannot_pause_replacement() {
        let sink = window_sink(Arc::new(Mutex::new(Vec::new())));
        let first = sink
            .attach_from(
                replay_sink(Arc::new(Mutex::new(Vec::new()))),
                PtyCursor::default(),
            )
            .unwrap();
        let second = sink
            .attach_from(
                replay_sink(Arc::new(Mutex::new(Vec::new()))),
                PtyCursor::default(),
            )
            .unwrap();

        assert!(!sink.pause_attachment(first));
        assert!(matches!(
            &sink.state.lock().subscriber,
            SubscriberState::Active(Subscriber::Replay { attachment, .. })
                if *attachment == second
        ));
        assert!(sink.pause_attachment(second));
    }
}
