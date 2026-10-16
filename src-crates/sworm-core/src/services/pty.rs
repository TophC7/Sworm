use crate::events::EventSink;
use crate::services::completed_runs::CompletedRun;
use parking_lot::{Condvar, Mutex};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sworm_protocol::pty::PtyEvent;
use sworm_protocol::rpc::PtyCursor;
use tracing::{error, info, warn};

const PTY_READ_BUF_SIZE: usize = 64 * 1024;
const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
const RETAINED_OUTPUT_CAP: usize = 8 * 1024 * 1024;
/// Coalescing ceiling for one retained output chunk. Trimming then drops whole
/// chunks off the front instead of memmoving the entire window every flush.
const RETAINED_CHUNK_CAP: usize = 256 * 1024;
static NEXT_ATTACHMENT: AtomicU64 = AtomicU64::new(1);
static NEXT_CHILD: AtomicU64 = AtomicU64::new(1);
pub(crate) const PROCESS_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// OS wait acknowledgement, independent of transcript/event finalization.
#[derive(Default)]
pub(crate) struct ChildExit {
    pub(crate) result: Mutex<Option<Result<Option<i32>, String>>>,
    pub(crate) changed: Condvar,
}

impl ChildExit {
    pub(crate) fn result(&self) -> Option<Result<Option<i32>, String>> {
        self.result.lock().clone()
    }

    fn wait(&self) -> Result<Option<i32>, String> {
        let mut result = self.result.lock();
        while result.is_none() {
            self.changed.wait(&mut result);
        }
        result.as_ref().expect("wait completed").clone()
    }

    fn pause_until(&self, deadline: Instant) {
        let mut result = self.result.lock();
        if result.is_none() {
            self.changed.wait_until(&mut result, deadline);
        } else {
            drop(result);
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
    }
}

#[cfg(unix)]
pub(crate) fn signal_process(target: libc::pid_t, signal: libc::c_int) -> Result<(), String> {
    if target == 0 || target == -1 || (target < 0 && -target == unsafe { libc::getpgrp() }) {
        return Err("Refusing to signal Sworm's own process group".to_string());
    }
    if unsafe { libc::kill(target, signal) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(format!("Failed to signal process {target}: {error}"))
    }
}

#[cfg(unix)]
pub(crate) fn groups_terminated(groups: &[libc::pid_t]) -> Result<bool, String> {
    for &group in groups {
        if group <= 1 || group == unsafe { libc::getpgrp() } {
            return Err("Refusing to inspect Sworm's own process group".to_string());
        }
        if unsafe { libc::kill(-group, 0) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            return Err(format!("Failed to inspect process group {group}: {error}"));
        }
        #[cfg(target_os = "linux")]
        {
            // An orphan zombie cannot access the checkout. The recorded leader
            // still requires its separate successful OS wait acknowledgement.
            for entry in std::fs::read_dir("/proc").map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry
                    .file_name()
                    .to_str()
                    .is_none_or(|name| name.parse::<u32>().is_err())
                {
                    continue;
                }
                let stat = match std::fs::read_to_string(entry.path().join("stat")) {
                    Ok(stat) => stat,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(format!("Failed to inspect process group: {error}")),
                };
                let (_, fields) = stat.rsplit_once(')').ok_or("Invalid /proc process stat")?;
                let mut fields = fields.split_whitespace();
                let state = fields.next().ok_or("Missing /proc process state")?;
                let process_group = fields
                    .nth(1)
                    .ok_or("Missing /proc process group")?
                    .parse::<libc::pid_t>()
                    .map_err(|error| error.to_string())?;
                if process_group == group && state != "Z" && state != "X" {
                    return Ok(false);
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        return Ok(false);
    }
    Ok(true)
}

/// Signal only the recorded leader and known child groups, then prove reaping
/// and group termination. A wait error is never an exit acknowledgement.
#[cfg(unix)]
pub(crate) fn stop_process(
    pid: Option<u32>,
    groups: &[libc::pid_t],
    exit: &ChildExit,
    deadline: Instant,
    mut poll_exit: impl FnMut(),
) -> Result<(), String> {
    let pid = pid
        .and_then(|pid| libc::pid_t::try_from(pid).ok())
        .filter(|pid| *pid > 1)
        .ok_or("Missing PTY/LSP child identity")?;
    let deadline = deadline.min(Instant::now() + PROCESS_STOP_TIMEOUT);
    poll_exit();
    match exit.result() {
        Some(Err(error)) => return Err(error),
        Some(Ok(_)) if groups_terminated(groups)? => return Ok(()),
        _ => {}
    }
    if Instant::now() >= deadline {
        return Err(format!("Timed out waiting for child {pid} to exit"));
    }
    let graceful = deadline.min(Instant::now() + Duration::from_secs(1));
    for signal in [libc::SIGTERM, libc::SIGKILL] {
        poll_exit();
        {
            // The reaper also takes this latch before try_wait/wait. Once it
            // acknowledges exit, the numeric PID must never be signalled again.
            let result = exit.result.lock();
            if let Some(Err(error)) = result.as_ref() {
                return Err(error.clone());
            }
            for &group in groups {
                if group <= 1 {
                    return Err(format!("Invalid child process group: {group}"));
                }
                signal_process(-group, signal)?;
            }
            if result.is_none() {
                signal_process(pid, signal)?;
            }
        }
        let phase_deadline = if signal == libc::SIGTERM {
            graceful
        } else {
            deadline
        };
        loop {
            poll_exit();
            match exit.result() {
                Some(Err(error)) => return Err(error),
                Some(Ok(_)) if groups_terminated(groups)? => return Ok(()),
                _ => {}
            }
            if Instant::now() >= phase_deadline {
                break;
            }
            exit.pause_until(phase_deadline.min(Instant::now() + Duration::from_millis(20)));
        }
    }
    Err(format!(
        "Timed out waiting for child {pid} and its process groups to exit"
    ))
}

/// Receives a finished retained transcript. Called outside PTY/stream locks.
pub type CompletedRunSink = Arc<dyn Fn(CompletedRun) + Send + Sync>;

#[derive(Clone)]
pub struct PtySubscriber {
    pub output: EventSink<Vec<u8>>,
    pub events: EventSink<PtyEvent>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunRetention {
    Retained,
    Ephemeral,
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
    Channels(PtySubscriber),
    Replay {
        attachment: u64,
        sink: EventSink<PtyReplay>,
        channels: Option<PtySubscriber>,
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
}

struct PtyStreamState {
    subscriber: SubscriberState,
    original: Option<(PtySubscriber, PtyCursor)>,
    target_cursor: Option<PtyCursor>,
    delivered_cursor: PtyCursor,
    transfer_pending: bool,
    retained: Vec<Retained>,
    retained_bytes: usize,
    overflow_warned: bool,
    last_dispatched: u64,
    sequence: u64,
    output_offset: u64,
    event_sequence: u64,
    retention: RunRetention,
    exit_code: Option<Option<i32>>,
}

impl PtyStreamState {
    fn retain(&mut self, sequence: u64, start_offset: u64, event_sequence: u64, payload: Payload) {
        match payload {
            Payload::Output(bytes) => {
                self.retained_bytes += bytes.len();
                let coalesce = matches!(
                    self.retained.last(),
                    Some(Retained {
                        payload: Payload::Output(retained),
                        ..
                    }) if retained.len() + bytes.len() <= RETAINED_CHUNK_CAP
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
                    }),
                }
            }
            Payload::Event(event) => self.retained.push(Retained {
                sequence,
                start_offset: 0,
                event_sequence,
                payload: Payload::Event(event),
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
    run_id: String,
    state: Arc<Mutex<PtyStreamState>>,
}

impl PtyEventSink {
    fn new(run_id: String, subscriber: Option<PtySubscriber>, retention: RunRetention) -> Self {
        Self {
            run_id,
            state: Arc::new(Mutex::new(PtyStreamState {
                subscriber: subscriber.map_or(SubscriberState::Paused, |channels| {
                    SubscriberState::Active(Subscriber::Channels(channels))
                }),
                original: None,
                target_cursor: None,
                delivered_cursor: PtyCursor::default(),
                transfer_pending: false,
                retained: Vec::new(),
                retained_bytes: 0,
                overflow_warned: false,
                last_dispatched: 0,
                sequence: 1,
                output_offset: 0,
                event_sequence: 0,
                retention,
                exit_code: None,
            })),
        }
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
        let sequence = state.sequence;
        state.sequence = state
            .sequence
            .checked_add(1)
            .expect("PTY sequence overflow");
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
            state.retain(sequence, start_offset, event_sequence, payload);
            return;
        }
        let transferring = state.transfer_pending;
        let SubscriberState::Active(subscriber) = &state.subscriber else {
            unreachable!("paused subscriber handled above");
        };
        let mut delivered_cursor = state.delivered_cursor;
        match &payload {
            Payload::Output(bytes) => {
                delivered_cursor.output_offset = start_offset + bytes.len() as u64;
            }
            Payload::Event(_) => delivered_cursor.event_sequence = event_sequence,
        }
        let retained = transferring || state.retention == RunRetention::Retained;
        let (delivered, payload) = if retained {
            (
                payload
                    .clone()
                    .send(subscriber, start_offset, event_sequence)
                    .is_ok(),
                Some(payload),
            )
        } else {
            (
                payload
                    .send(subscriber, start_offset, event_sequence)
                    .is_ok(),
                None,
            )
        };
        if delivered {
            state.last_dispatched = sequence;
            state.delivered_cursor = delivered_cursor;
            if transferring {
                state.target_cursor = Some(state.delivered_cursor);
            }
        } else {
            state.subscriber = SubscriberState::Paused;
            warn!("PTY subscriber channel closed");
        }
        if let Some(payload) = payload {
            state.retain(sequence, start_offset, event_sequence, payload);
        }
    }

    fn complete(&self, code: Option<i32>) -> bool {
        let mut state = self.state.lock();
        state.exit_code = Some(code);
        self.emit_payload_locked(
            &mut state,
            Payload::Event(PtyEvent::Exit {
                run_id: self.run_id.clone(),
                code,
            }),
        );
        state.retention == RunRetention::Retained
            || state.transfer_pending
            || matches!(&state.subscriber, SubscriberState::Paused)
    }

    #[cfg(test)]
    fn pause(&self) -> u64 {
        self.pause_state(&mut self.state.lock())
    }

    fn pause_state(&self, state: &mut PtyStreamState) -> u64 {
        if state.transfer_pending {
            return state.last_dispatched;
        }
        let channels = match &state.subscriber {
            SubscriberState::Active(Subscriber::Channels(channels)) => Some(channels.clone()),
            SubscriberState::Active(Subscriber::Replay { channels, .. }) => channels.clone(),
            SubscriberState::Paused => None,
        };
        if let Some(channels) = channels {
            state.original = Some((channels, state.delivered_cursor));
        }
        state.target_cursor = Some(state.delivered_cursor);
        state.transfer_pending = true;
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

    fn emit_synced(
        &self,
        state: &mut PtyStreamState,
        subscriber: &Subscriber,
    ) -> Result<(), String> {
        let event = PtyEvent::Synced {
            run_id: self.run_id.clone(),
            sequence: state.sequence.saturating_sub(1),
        };
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

    /// Deliver retained history to either a cursor-aware stream or native channels.
    /// Advance each cursor only after its subscriber accepts the delivery.
    fn replay_to(
        &self,
        state: &mut PtyStreamState,
        subscriber: &Subscriber,
        cursor: &mut PtyCursor,
    ) -> Result<(), String> {
        if let Some(first_offset) = state.retained.iter().find_map(|entry| {
            matches!(&entry.payload, Payload::Output(bytes) if !bytes.is_empty())
                .then_some(entry.start_offset)
        }) {
            if cursor.output_offset < first_offset {
                let lost_bytes = first_offset - cursor.output_offset;
                let result = match subscriber {
                    Subscriber::Channels(channels) => (channels.events)(PtyEvent::Error {
                        run_id: self.run_id.clone(),
                        message: format!(
                            "{lost_bytes} bytes of output were lost while disconnected"
                        ),
                    }),
                    Subscriber::Replay { sink, .. } => sink(PtyReplay::Gap { lost_bytes }),
                };
                result.map_err(|_| "PTY channel closed while sending gap".to_string())?;
                cursor.output_offset = first_offset;
            }
        }
        for entry in &state.retained {
            match &entry.payload {
                Payload::Output(bytes) => {
                    let skip = cursor.output_offset.saturating_sub(entry.start_offset);
                    let skip = usize::try_from(skip).unwrap_or(usize::MAX).min(bytes.len());
                    if skip == bytes.len() {
                        continue;
                    }
                    let start = entry.start_offset + skip as u64;
                    Payload::Output(bytes[skip..].to_vec()).send(subscriber, start, 0)?;
                    cursor.output_offset = entry.start_offset + bytes.len() as u64;
                }
                Payload::Event(event) => {
                    if entry.event_sequence <= cursor.event_sequence {
                        continue;
                    }
                    Payload::Event(event.clone()).send(subscriber, 0, entry.event_sequence)?;
                    cursor.event_sequence = entry.event_sequence;
                }
            }
            state.last_dispatched = entry.sequence;
        }
        Ok(())
    }

    fn attach(&self, channels: PtySubscriber) -> Result<u64, String> {
        let mut state = self.state.lock();
        if !state.transfer_pending {
            return Err("PTY has no pending transfer".to_string());
        }
        let subscriber = Subscriber::Channels(channels);
        state.subscriber = SubscriberState::Active(subscriber.clone());
        let mut cursor = state
            .target_cursor
            .expect("pending transfer has target cursor");
        let replay = self.replay_to(&mut state, &subscriber, &mut cursor);
        state.target_cursor = Some(cursor);
        state.delivered_cursor = cursor;
        if let Err(error) = replay {
            state.subscriber = SubscriberState::Paused;
            return Err(error);
        }
        self.emit_synced(&mut state, &subscriber)?;
        Ok(state.sequence.saturating_sub(1))
    }

    fn attach_from(&self, sink: EventSink<PtyReplay>, cursor: PtyCursor) -> Result<u64, String> {
        let mut state = self.state.lock();
        self.attach_from_state(&mut state, sink, cursor, None)
    }

    fn attach_from_state(
        &self,
        state: &mut PtyStreamState,
        sink: EventSink<PtyReplay>,
        cursor: PtyCursor,
        channels: Option<PtySubscriber>,
    ) -> Result<u64, String> {
        if state.retention != RunRetention::Retained {
            return Err("Cursor attachment requires a retained PTY".to_string());
        }
        if state.transfer_pending {
            return Err("PTY transfer is pending".to_string());
        }
        // Tokens must not restart at one when a run ID is reused: a late
        // stream from the previous incarnation must never pause the new one.
        let attachment = NEXT_ATTACHMENT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                next.checked_add(1)
            })
            .expect("PTY attachment sequence overflow");
        let subscriber = Subscriber::Replay {
            attachment,
            sink: sink.clone(),
            channels,
        };
        state.subscriber = SubscriberState::Active(subscriber.clone());

        let mut delivered = cursor;
        let replay = self.replay_to(state, &subscriber, &mut delivered);
        state.delivered_cursor = delivered;
        if let Err(error) = replay {
            state.subscriber = SubscriberState::Paused;
            return Err(error);
        }
        self.emit_synced(state, &subscriber)?;
        Ok(attachment)
    }

    fn resume_original(&self) -> Result<u64, String> {
        let mut state = self.state.lock();
        let (channels, mut cursor) = state
            .original
            .clone()
            .ok_or_else(|| "PTY has no original subscriber to restore".to_string())?;
        let subscriber = Subscriber::Channels(channels);
        state.subscriber = SubscriberState::Active(subscriber.clone());
        let replay = self.replay_to(&mut state, &subscriber, &mut cursor);
        state.original.as_mut().expect("original present").1 = cursor;
        state.delivered_cursor = cursor;
        if let Err(error) = replay {
            state.subscriber = SubscriberState::Paused;
            return Err(error);
        }
        self.emit_synced(&mut state, &subscriber)?;
        state.original = None;
        state.target_cursor = None;
        state.transfer_pending = false;
        if state.retention == RunRetention::Ephemeral {
            state.clear_retained();
        }
        Ok(state.sequence.saturating_sub(1))
    }

    fn abort_transfer_detached(&self) -> Result<(), String> {
        let mut state = self.state.lock();
        if state.retention != RunRetention::Retained || !state.transfer_pending {
            return Err("PTY has no pending retained transfer".to_string());
        }
        state.subscriber = SubscriberState::Paused;
        state.original = None;
        state.target_cursor = None;
        state.transfer_pending = false;
        Ok(())
    }

    fn commit_transfer(&self) {
        let mut state = self.state.lock();
        state.original = None;
        state.target_cursor = None;
        state.transfer_pending = false;
        if state.retention == RunRetention::Ephemeral {
            state.clear_retained();
        }
    }

    fn is_completed(&self) -> bool {
        self.state.lock().exit_code.is_some()
    }

    fn exit_code(&self) -> Option<Option<i32>> {
        self.state.lock().exit_code
    }

    /// Drain retained history into a transcript for durable storage.
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
            // A run without output starts where its output cursor ended.
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
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    pid: Option<u32>,
    exit: ChildExit,
    stop: Mutex<()>,
    #[cfg(unix)]
    groups: Mutex<Vec<libc::pid_t>>,
}

impl LocalBackend {
    fn poll_exit(&self) {
        let mut result = self.exit.result.lock();
        if result.is_some() {
            return;
        }
        let mut child = self.child.lock();
        let waited = match child.try_wait() {
            Ok(None) => return,
            Ok(Some(_)) => child.wait().map(|status| Some(status.exit_code() as i32)),
            Err(error) => Err(error),
        };
        // Publish immediately after OS wait, never after reader finalization.
        *result = Some(waited.map_err(|error| format!("Failed waiting for PTY child: {error}")));
        self.exit.changed.notify_all();
    }

    fn stop(&self, deadline: Instant) -> Result<(), String> {
        let _stop = self
            .stop
            .try_lock_until(deadline)
            .ok_or("Timed out waiting for an already-stopping PTY child")?;
        #[cfg(unix)]
        {
            let mut groups = self.groups.lock();
            if groups.is_empty() && matches!(self.exit.result(), Some(Ok(_))) {
                return Ok(());
            }
            if let Some(group) = self.master.lock().process_group_leader() {
                if !groups.contains(&group) {
                    groups.push(group);
                }
            }
            let stopped =
                stop_process(self.pid, &groups, &self.exit, deadline, || self.poll_exit());
            if stopped.is_ok() {
                groups.clear();
            }
            stopped
        }
        #[cfg(not(unix))]
        {
            if self.exit.result().is_none() {
                self.child
                    .lock()
                    .kill()
                    .map_err(|error| error.to_string())?;
            }
            while self.exit.result().is_none() && Instant::now() < deadline {
                self.poll_exit();
                self.exit
                    .pause_until(deadline.min(Instant::now() + Duration::from_millis(20)));
            }
            self.exit
                .result()
                .ok_or("Timed out waiting for PTY child")?
                .map(|_| ())
        }
    }

    fn has_exited(&self) -> bool {
        if !matches!(self.exit.result(), Some(Ok(_))) {
            return false;
        }
        #[cfg(unix)]
        {
            let mut groups = self.groups.lock();
            if !groups.is_empty() {
                if let Some(group) = self.master.lock().process_group_leader() {
                    if !groups.contains(&group) {
                        groups.push(group);
                    }
                }
            }
            if groups_terminated(&groups).unwrap_or(false) {
                groups.clear();
                return true;
            }
            false
        }
        #[cfg(not(unix))]
        true
    }
}

struct PendingPtyExit {
    run_id: String,
    root: PathBuf,
    backend: Arc<LocalBackend>,
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
        self.stop(Instant::now() + PROCESS_STOP_TIMEOUT)
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

/// Stable incarnation. Removing the map entry retires this slot; slow startup
/// cannot publish into another run with the same ID.
struct RunEntry {
    state: Mutex<RunEntryState>,
}

struct RunEntryState {
    owner_id: Option<String>,
    project_root: Option<PathBuf>,
    run: RunSlot,
}

enum RunSlot {
    Reserved,
    Active(Arc<LivePty>),
    Retired,
}

struct LivePty {
    backend: Arc<dyn RunBackend>,
    shutdown: Arc<AtomicBool>,
    finalized: Arc<AtomicBool>,
    local: Option<Arc<LocalBackend>>,
    event_sink: PtyEventSink,
}

impl LivePty {
    fn fence(&self) {
        self.shutdown.store(true, Ordering::Release);
        self.finalized.store(true, Ordering::Release);
    }
}

/// PTY service managing live runs and completed retained transcripts.
pub struct PtyService {
    sessions: Arc<Mutex<HashMap<String, Arc<RunEntry>>>>,
    pending_exits: Arc<Mutex<HashMap<u64, Arc<PendingPtyExit>>>>,
    #[cfg(test)]
    startup_hook: Mutex<Option<Arc<dyn Fn(&str, Option<u32>) + Send + Sync>>>,
}

struct StartupReservation<'a> {
    service: &'a PtyService,
    run_id: &'a str,
    entry: Arc<RunEntry>,
}

impl Drop for StartupReservation<'_> {
    fn drop(&mut self) {
        // Publication replaces Reserved. A failed start removes only its own
        // incarnation; a newer run with the same ID is never disturbed.
        let mut state = self.entry.state.lock();
        let mut sessions = self.service.sessions.lock();
        if sessions
            .get(self.run_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.entry))
            && matches!(state.run, RunSlot::Reserved)
        {
            state.run = RunSlot::Retired;
            sessions.remove(self.run_id);
        }
    }
}
impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            pending_exits: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            startup_hook: Mutex::new(None),
        }
    }

    fn reserve(&self, run_id: &str, owner_id: Option<String>) -> Result<Arc<RunEntry>, String> {
        let mut sessions = self.sessions.lock();
        if sessions.contains_key(run_id) {
            return Err(format!("PTY session already exists: {run_id}"));
        }
        let entry = Arc::new(RunEntry {
            state: Mutex::new(RunEntryState {
                owner_id,
                project_root: None,
                run: RunSlot::Reserved,
            }),
        });
        sessions.insert(run_id.to_string(), Arc::clone(&entry));
        Ok(entry)
    }

    fn entry(&self, run_id: &str) -> Result<Arc<RunEntry>, String> {
        self.sessions
            .lock()
            .get(run_id)
            .cloned()
            .ok_or_else(|| format!("No PTY session: {run_id}"))
    }

    fn active(&self, run_id: &str) -> Result<Arc<LivePty>, String> {
        let entry = self.entry(run_id)?;
        Self::active_entry(&entry, run_id)
    }

    fn active_entry(entry: &RunEntry, run_id: &str) -> Result<Arc<LivePty>, String> {
        let state = entry.state.lock();
        match &state.run {
            RunSlot::Active(live) => Ok(Arc::clone(live)),
            _ => Err(format!("No active PTY session: {run_id}")),
        }
    }
    fn validate_owner(
        entry: &RunEntry,
        live: &Arc<LivePty>,
        run_id: &str,
        owner_id: Option<&str>,
    ) -> Result<(), String> {
        let state = entry.state.lock();
        if state.owner_id.as_deref() != owner_id {
            return Err(format!(
                "PTY session belongs to a different owner: {run_id}"
            ));
        }
        if !matches!(&state.run, RunSlot::Active(current) if Arc::ptr_eq(current, live)) {
            return Err(format!("No active PTY session: {run_id}"));
        }
        Ok(())
    }

    fn publish(
        &self,
        run_id: &str,
        entry: &Arc<RunEntry>,
        live: Arc<LivePty>,
    ) -> Result<(), String> {
        let mut state = entry.state.lock();
        let sessions = self.sessions.lock();
        if !sessions
            .get(run_id)
            .is_some_and(|current| Arc::ptr_eq(current, entry))
            || !matches!(state.run, RunSlot::Reserved)
        {
            return Err(format!("PTY startup cancelled: {run_id}"));
        }
        state.run = RunSlot::Active(live);
        Ok(())
    }

    fn remove_if_current(
        sessions: &Mutex<HashMap<String, Arc<RunEntry>>>,
        run_id: &str,
        entry: &Arc<RunEntry>,
    ) -> Option<Arc<LivePty>> {
        let mut state = entry.state.lock();
        let mut sessions = sessions.lock();
        if !sessions
            .get(run_id)
            .is_some_and(|current| Arc::ptr_eq(current, entry))
        {
            return None;
        }
        let live = match std::mem::replace(&mut state.run, RunSlot::Retired) {
            RunSlot::Active(live) => Some(live),
            _ => None,
        };
        sessions.remove(run_id);
        live
    }

    /// Spawn a new local PTY. Duplicate IDs are rejected without touching the existing process.
    pub fn spawn(
        &self,
        run_id: String,
        project_root: &Path,
        cmd: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        cols: u16,
        rows: u16,
        subscriber: Option<PtySubscriber>,
        owner_id: Option<String>,
        on_exit: Option<Box<dyn FnOnce(&str, Option<i32>) + Send>>,
        completed: Option<CompletedRunSink>,
    ) -> Result<PtyEventSink, String> {
        let project_root = project_root
            .canonicalize()
            .map_err(|error| format!("Failed to resolve PTY project folder: {error}"))?;
        let entry = self.reserve(&run_id, owner_id)?;
        entry.state.lock().project_root = Some(project_root.clone());
        let reservation = StartupReservation {
            service: self,
            run_id: &run_id,
            entry: Arc::clone(&entry),
        };
        #[cfg(test)]
        let hook = self.startup_hook.lock().clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(&run_id, None);
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
        // Resolve all fallible PTY handles before there is a child to orphan.
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("Failed to get PTY writer: {error}"))?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("Failed to clone PTY reader: {error}"))?;

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

        let child = pair
            .slave
            .spawn_command(cmd_builder)
            .map_err(|error| format!("Failed to spawn process: {error}"))?;
        let pid = child.process_id();
        #[cfg(unix)]
        let leader_group = pid.and_then(|pid| {
            let group = unsafe { libc::getpgid(pid as libc::pid_t) };
            (group > 1).then_some(group)
        });
        let backend = Arc::new(LocalBackend {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            pid,
            exit: ChildExit::default(),
            stop: Mutex::new(()),
            #[cfg(unix)]
            groups: Mutex::new(leader_group.into_iter().collect()),
        });
        let child_id = NEXT_CHILD.fetch_add(1, Ordering::Relaxed);
        self.pending_exits.lock().insert(
            child_id,
            Arc::new(PendingPtyExit {
                run_id: run_id.clone(),
                root: project_root,
                backend: Arc::clone(&backend),
            }),
        );
        #[cfg(test)]
        let hook = self.startup_hook.lock().clone();
        #[cfg(test)]
        if let Some(hook) = hook {
            hook(&run_id, pid);
        }
        let reaper = Arc::clone(&backend);
        let pending_exits = Arc::clone(&self.pending_exits);
        if let Err(error) = std::thread::Builder::new()
            .name(format!("pty-wait-{run_id}"))
            .spawn(move || loop {
                reaper.poll_exit();
                match reaper.exit.result() {
                    Some(Err(_)) => break,
                    Some(Ok(_)) if reaper.has_exited() => {
                        pending_exits.lock().remove(&child_id);
                        break;
                    }
                    _ => {}
                }
                std::thread::sleep(Duration::from_millis(20));
            })
        {
            let stopped = backend.stop(Instant::now() + PROCESS_STOP_TIMEOUT);
            if stopped.is_ok() {
                self.pending_exits.lock().remove(&child_id);
            }
            return Err(format!(
                "Failed to spawn PTY wait thread: {error}; cleanup: {stopped:?}"
            ));
        }

        let shutdown = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));
        let event_sink = PtyEventSink::new(run_id.clone(), subscriber, RunRetention::Retained);
        let live = Arc::new(LivePty {
            backend: backend.clone(),
            local: Some(Arc::clone(&backend)),
            shutdown: Arc::clone(&shutdown),
            finalized: Arc::clone(&finalized),
            event_sink: event_sink.clone(),
        });
        // The reader waits for publication; OS reaping is independent of reads.
        let exit_for_thread = Arc::clone(&backend);
        let (start_tx, start_rx) = std::sync::mpsc::sync_channel::<()>(0);

        let sid_for_thread = run_id.clone();
        let sessions_for_thread = Arc::clone(&self.sessions);
        let entry_for_thread = Arc::clone(&entry);
        let sink_for_thread = event_sink.clone();

        let started = std::thread::Builder::new()
            .name(format!("pty-reader-{run_id}"))
            .spawn(move || {
                if start_rx.recv().is_err() {
                    return;
                }
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

                let exit_code = match exit_for_thread.exit.wait() {
                    Ok(code) => code,
                    Err(error) => {
                        warn!("Failed waiting for PTY child {sid_for_thread}: {error}");
                        None
                    }
                };

                if finalized.swap(true, Ordering::AcqRel) {
                    info!("Skipping duplicate PTY finalization for {sid_for_thread}");
                    return;
                }
                if let Some(callback) = on_exit {
                    callback(&sid_for_thread, exit_code);
                }

                let retain = sink_for_thread.complete(exit_code);
                if retain {
                    if let Some(archive) = completed {
                        archive(sink_for_thread.take_completed());
                        info!("PTY {sid_for_thread} exited; transcript stored");
                    } else {
                        info!("PTY {sid_for_thread} exited; retaining transcript");
                        return;
                    }
                }

                Self::remove_if_current(&sessions_for_thread, &sid_for_thread, &entry_for_thread);
            });
        if let Err(error) = started {
            let stopped = backend.stop(Instant::now() + PROCESS_STOP_TIMEOUT);
            if stopped.is_ok() {
                self.pending_exits.lock().remove(&child_id);
            }
            return Err(format!(
                "Failed to spawn reader thread: {error}; cleanup: {stopped:?}"
            ));
        }
        if let Err(error) = self.publish(&run_id, &entry, Arc::clone(&live)) {
            live.fence();
            let stopped = backend.stop(Instant::now() + PROCESS_STOP_TIMEOUT);
            if stopped.is_ok() {
                self.pending_exits.lock().remove(&child_id);
            }
            return Err(error);
        }
        drop(reservation);
        event_sink.emit(PtyEvent::Started {
            run_id: run_id.clone(),
            pid,
        });
        let _ = start_tx.send(());

        Ok(event_sink)
    }

    /// Adopt a non-local run. Duplicate ids are rejected instead of replacing
    /// the existing run because replacement could orphan its remote process.
    pub fn adopt(
        &self,
        run_id: String,
        backend: Box<dyn RunBackend>,
        subscriber: Option<PtySubscriber>,
        owner_id: Option<String>,
    ) -> Result<PtyEventSink, String> {
        let entry = self.reserve(&run_id, owner_id)?;
        let event_sink = PtyEventSink::new(run_id.clone(), subscriber, RunRetention::Ephemeral);
        let live = Arc::new(LivePty {
            backend: Arc::from(backend),
            local: None,
            shutdown: Arc::new(AtomicBool::new(false)),
            finalized: Arc::new(AtomicBool::new(false)),
            event_sink: event_sink.clone(),
        });
        self.publish(&run_id, &entry, live)?;
        Ok(event_sink)
    }

    /// Pause a native run only while its current owner still holds it.
    pub fn pause_owned(&self, run_id: &str, owner_id: &str) -> Result<u64, String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        let mut stream = live.event_sink.state.lock();
        Self::validate_owner(&entry, &live, run_id, Some(owner_id))?;
        if stream.transfer_pending {
            return Err(format!("PTY transfer is pending: {run_id}"));
        }
        Ok(live.event_sink.pause_state(&mut stream))
    }

    /// Pause only if `attachment` still owns the replay subscriber. A stale
    /// stream cannot detach a newer replacement stream during cleanup.
    pub fn pause_attachment(&self, run_id: &str, attachment: u64) -> Result<bool, String> {
        Ok(self.active(run_id)?.event_sink.pause_attachment(attachment))
    }

    /// Attach the target of a pending native transfer (retained locals or ephemeral proxies).
    pub fn attach(&self, run_id: &str, subscriber: PtySubscriber) -> Result<u64, String> {
        self.active(run_id)?.event_sink.attach(subscriber)
    }

    /// Check native ownership without replacing its current subscriber.
    pub fn ensure_owner(&self, run_id: &str, owner_id: Option<&str>) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        let stream = live.event_sink.state.lock();
        Self::validate_owner(&entry, &live, run_id, owner_id)?;
        if stream.transfer_pending {
            return Err(format!("PTY transfer is pending: {run_id}"));
        }
        Ok(())
    }

    /// Validate the source owner of a native transfer while attachment is paused.
    pub fn ensure_transfer_owner(&self, run_id: &str, owner_id: &str) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        let stream = live.event_sink.state.lock();
        Self::validate_owner(&entry, &live, run_id, Some(owner_id))?;
        if !stream.transfer_pending {
            return Err(format!("PTY transfer is not pending: {run_id}"));
        }
        Ok(())
    }

    /// Reattach a matching retained run after its desktop view was replaced.
    /// Replays from the beginning, including original Started/Exit, before live delivery.
    pub fn attach_same_owner(
        &self,
        run_id: &str,
        owner_id: Option<&str>,
        subscriber: PtySubscriber,
        cols: u16,
        rows: u16,
    ) -> Result<PtyRunState, String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        let event_sink = &live.event_sink;
        let status = {
            let state = event_sink.state.lock();
            Self::validate_owner(&entry, &live, run_id, owner_id)?;
            if state.transfer_pending {
                return Err(format!("PTY transfer is pending: {run_id}"));
            }
            if state.retention != RunRetention::Retained {
                return Err(format!("PTY session has no retained replay: {run_id}"));
            }
            state
                .exit_code
                .map_or(PtyRunState::Live, PtyRunState::Completed)
        };
        if status == PtyRunState::Live {
            live.backend.resize(cols, rows)?;
        }
        let replay_run_id = run_id.to_owned();
        let replay_channels = subscriber.clone();
        let replay: EventSink<PtyReplay> = Arc::new(move |frame| match frame {
            PtyReplay::Gap { lost_bytes } => (subscriber.events)(PtyEvent::Error {
                run_id: replay_run_id.clone(),
                message: format!("{lost_bytes} bytes of output were lost while disconnected"),
            }),
            PtyReplay::Output { bytes, .. } => (subscriber.output)(bytes),
            PtyReplay::Event { event, .. } => (subscriber.events)(event),
        });
        let mut state = event_sink.state.lock();
        Self::validate_owner(&entry, &live, run_id, owner_id)?;
        event_sink.attach_from_state(
            &mut state,
            replay,
            PtyCursor::default(),
            Some(replay_channels),
        )?;
        Ok(status)
    }

    /// Attach a cursor-aware subscriber to a retained run.
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
        self.active(run_id)?.event_sink.attach_from(sink, cursor)
    }

    /// Cursor immediately after all output and events emitted by a known run.
    pub fn cursor(&self, run_id: &str) -> Option<PtyCursor> {
        let live = self.active(run_id).ok()?;
        let state = live.event_sink.state.lock();
        Some(PtyCursor {
            output_offset: state.output_offset,
            event_sequence: state.event_sequence,
        })
    }

    /// Distinguish live, completed-with-replay, and unknown runs.
    pub fn run_state(&self, run_id: &str) -> Option<PtyRunState> {
        let live = self.active(run_id).ok()?;
        Some(
            live.event_sink
                .exit_code()
                .map_or(PtyRunState::Live, PtyRunState::Completed),
        )
    }
    /// Complete an adopted run after its backend reports Exit.
    pub fn complete_adopted(&self, run_id: &str, code: Option<i32>) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        if !live.backend.detaches_on_shutdown() {
            return Err(format!("PTY session is not adopted: {run_id}"));
        }
        if live.finalized.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let retain = live.event_sink.complete(code);
        if !retain {
            Self::remove_if_current(&self.sessions, run_id, &entry);
        }
        Ok(())
    }

    /// Abort a transfer and restore the callbacks detached by `pause`.
    pub fn resume_original(&self, run_id: &str) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let sink = Self::active_entry(&entry, run_id)?.event_sink.clone();
        sink.resume_original()?;
        if sink.is_completed() && sink.state.lock().retention == RunRetention::Ephemeral {
            Self::remove_if_current(&self.sessions, run_id, &entry);
        }
        Ok(())
    }

    /// Drop failed native rollback channels, retaining the local run and its
    /// history for a later same-owner view attachment.
    pub fn abort_transfer_detached(&self, run_id: &str) -> Result<(), String> {
        self.active(run_id)?.event_sink.abort_transfer_detached()
    }

    /// Change native ownership without discarding rollback state. The caller commits
    /// separately after every part of the transfer has succeeded.
    pub fn transfer_owner(&self, run_id: &str, new_owner_id: &str) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        let stream = live.event_sink.state.lock();
        let mut state = entry.state.lock();
        if !matches!(&state.run, RunSlot::Active(current) if Arc::ptr_eq(current, &live)) {
            return Err(format!("No active PTY session: {run_id}"));
        }
        if !stream.transfer_pending {
            return Err(format!("PTY transfer is not pending: {run_id}"));
        }
        state.owner_id = Some(new_owner_id.to_string());
        Ok(())
    }

    /// Move an adopted proxy without a native subscriber transfer. Its daemon
    /// retains the process; the target replaces the view from daemon replay.
    pub fn reassign_adopted(&self, run_id: &str, source: &str, target: &str) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        if !live.backend.detaches_on_shutdown() {
            return Err(format!("PTY session is not adopted: {run_id}"));
        }
        let stream = live.event_sink.state.lock();
        let mut state = entry.state.lock();
        if state.owner_id.as_deref() != Some(source)
            || !matches!(&state.run, RunSlot::Active(current) if Arc::ptr_eq(current, &live))
        {
            return Err(format!(
                "PTY session belongs to a different owner: {run_id}"
            ));
        }
        if stream.transfer_pending {
            return Err(format!("PTY transfer is pending: {run_id}"));
        }
        state.owner_id = Some(target.to_owned());
        Ok(())
    }

    /// Discard rollback data once a transfer no longer needs its original sink.
    pub fn commit_transfer(&self, run_id: &str) {
        if let Ok(entry) = self.entry(run_id) {
            if let Ok(live) = Self::active_entry(&entry, run_id) {
                live.event_sink.commit_transfer();
                if live.event_sink.is_completed()
                    && live.event_sink.state.lock().retention == RunRetention::Ephemeral
                {
                    Self::remove_if_current(&self.sessions, run_id, &entry);
                }
            }
        }
    }

    /// Write data to a live run.
    pub fn write(&self, run_id: &str, data: &[u8]) -> Result<(), String> {
        let live = self.active(run_id)?;
        if live.event_sink.is_completed() || live.shutdown.load(Ordering::Acquire) {
            return Err(format!("No active PTY session: {run_id}"));
        }
        live.backend.write(data)
    }

    /// Resize a live run.
    pub fn resize(&self, run_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let live = self.active(run_id)?;
        if live.event_sink.is_completed() || live.shutdown.load(Ordering::Acquire) {
            return Err(format!("No active PTY session: {run_id}"));
        }
        live.backend.resize(cols, rows)
    }

    /// Retire a desktop proxy before reopening its daemon stream from cursor zero.
    /// The daemon still owns the physical process; never send it Stop here.
    pub fn detach_adopted(&self, run_id: &str, owner_id: Option<&str>) -> Result<(), String> {
        let entry = self.entry(run_id)?;
        let live = Self::active_entry(&entry, run_id)?;
        if !live.backend.detaches_on_shutdown() {
            return Err(format!("PTY session is not adopted: {run_id}"));
        }
        let mut stream = live.event_sink.state.lock();
        let mut state = entry.state.lock();
        if state.owner_id.as_deref() != owner_id {
            return Err(format!(
                "PTY session belongs to a different owner: {run_id}"
            ));
        }
        if !matches!(&state.run, RunSlot::Active(current) if Arc::ptr_eq(current, &live)) {
            return Err(format!("No active PTY session: {run_id}"));
        }
        if stream.transfer_pending {
            return Err(format!("PTY transfer is pending: {run_id}"));
        }
        let mut sessions = self.sessions.lock();
        if !sessions
            .get(run_id)
            .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            return Err(format!("No PTY session: {run_id}"));
        }
        state.run = RunSlot::Retired;
        sessions.remove(run_id);
        drop(sessions);
        drop(state);
        stream.subscriber = SubscriberState::Paused;
        stream.clear_retained();
        drop(stream);
        live.fence();
        Ok(())
    }

    /// Kill a run explicitly. This always kills an adopted backend.
    pub fn kill(&self, run_id: &str) -> Result<(), String> {
        self.kill_until(run_id, Instant::now() + PROCESS_STOP_TIMEOUT)
    }

    fn kill_until(&self, run_id: &str, deadline: Instant) -> Result<(), String> {
        if let Ok(entry) = self.entry(run_id) {
            if let Ok(live) = Self::active_entry(&entry, run_id) {
                Self::stop_live(&live, true, deadline)?;
            }
            Self::remove_if_current(&self.sessions, run_id, &entry);
        }
        // An earlier stop/discard may have retired the active entry already.
        let pending: Vec<_> = self
            .pending_exits
            .lock()
            .iter()
            .filter(|(_, pending)| pending.run_id == run_id)
            .map(|(id, pending)| (*id, Arc::clone(pending)))
            .collect();
        for (id, pending) in pending {
            pending.backend.stop(deadline)?;
            self.pending_exits.lock().remove(&id);
        }
        info!("PTY session {run_id} stopped and reaped");
        Ok(())
    }

    /// Called under Host's exclusive admission. Project association, not task
    /// cwd or current subscriber ownership, selects all retained incarnations.
    pub fn drain_roots(&self, roots: &[PathBuf], deadline: Instant) -> Result<(), String> {
        let deadline = deadline.min(Instant::now() + PROCESS_STOP_TIMEOUT);
        let run_ids: Vec<String> = self
            .entries()
            .into_iter()
            .filter_map(|(id, entry)| {
                let state = entry.state.lock();
                state
                    .project_root
                    .as_ref()
                    .is_some_and(|folder| roots.iter().any(|root| folder.starts_with(root)))
                    .then_some(id)
            })
            .collect();
        let pending: Vec<_> = self
            .pending_exits
            .lock()
            .iter()
            .filter(|(_, pending)| roots.iter().any(|root| pending.root.starts_with(root)))
            .map(|(id, pending)| (*id, Arc::clone(pending)))
            .collect();
        for (id, pending) in pending {
            // Stop this incarnation, not a reused textual run ID in a different
            // project. The record outlives active-map removal and wait errors.
            pending.backend.stop(deadline)?;
            self.pending_exits.lock().remove(&id);
        }
        for run_id in &run_ids {
            if let Ok(entry) = self.entry(run_id) {
                let affected = entry
                    .state
                    .lock()
                    .project_root
                    .as_ref()
                    .is_some_and(|folder| roots.iter().any(|root| folder.starts_with(root)));
                if affected {
                    if let Ok(live) = Self::active_entry(&entry, run_id) {
                        Self::stop_live(&live, true, deadline)?;
                    }
                    Self::remove_if_current(&self.sessions, run_id, &entry);
                }
            }
        }
        Ok(())
    }

    fn stop_live(live: &LivePty, kill_adopted: bool, deadline: Instant) -> Result<(), String> {
        if let Some(local) = &live.local {
            local.stop(deadline)?;
        } else if (kill_adopted || !live.backend.detaches_on_shutdown())
            && !live.event_sink.is_completed()
        {
            live.backend.kill()?;
        }
        live.fence();
        Ok(())
    }

    /// Snapshot owner IDs including startup reservations; caller must recheck
    /// under its run gate before acting because ownership can change.
    pub fn owner_run_ids(&self, owner_id: &str, protected: &HashSet<String>) -> Vec<String> {
        let entries: Vec<_> = self
            .sessions
            .lock()
            .iter()
            .filter(|(id, _)| !protected.contains(*id))
            .map(|(id, entry)| (id.clone(), Arc::clone(entry)))
            .collect();
        entries
            .into_iter()
            .filter_map(|(id, entry)| {
                (entry.state.lock().owner_id.as_deref() == Some(owner_id)).then_some(id)
            })
            .collect()
    }

    fn retire_owned(
        &self,
        run_id: &str,
        owner_id: &str,
        protected: &HashSet<String>,
    ) -> Option<Option<Arc<LivePty>>> {
        if protected.contains(run_id) {
            return None;
        }
        let entry = self.entry(run_id).ok()?;
        let live = {
            let state = entry.state.lock();
            if state.owner_id.as_deref() != Some(owner_id) {
                return None;
            }
            match &state.run {
                RunSlot::Active(live) => Some(Arc::clone(live)),
                RunSlot::Reserved => None,
                RunSlot::Retired => return None,
            }
        };
        let stream = live.as_ref().map(|live| live.event_sink.state.lock());
        let mut state = entry.state.lock();
        let same_live = match (&state.run, &live) {
            (RunSlot::Active(current), Some(expected)) => Arc::ptr_eq(current, expected),
            (RunSlot::Reserved, None) => true,
            _ => false,
        };
        if state.owner_id.as_deref() != Some(owner_id)
            || !same_live
            || stream
                .as_ref()
                .is_some_and(|stream| stream.transfer_pending)
        {
            return None;
        }
        let mut sessions = self.sessions.lock();
        if !sessions
            .get(run_id)
            .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            return None;
        }
        let live = match std::mem::replace(&mut state.run, RunSlot::Retired) {
            RunSlot::Active(live) => Some(live),
            _ => None,
        };
        sessions.remove(run_id);
        Some(live)
    }

    pub fn kill_owned_run(
        &self,
        run_id: &str,
        owner_id: &str,
        protected: &HashSet<String>,
    ) -> bool {
        let Some(live) = self.retire_owned(run_id, owner_id, protected) else {
            return false;
        };
        if let Some(live) = live {
            if let Err(error) = Self::stop_live(&live, true, Instant::now() + PROCESS_STOP_TIMEOUT)
            {
                warn!("Failed to kill PTY {run_id} on owner close: {error}");
            }
        }
        true
    }

    pub fn detach_owned_run(
        &self,
        run_id: &str,
        owner_id: &str,
        protected: &HashSet<String>,
    ) -> bool {
        let Some(live) = self.retire_owned(run_id, owner_id, protected) else {
            return false;
        };
        if let Some(live) = live {
            if let Err(error) = Self::stop_live(&live, false, Instant::now() + PROCESS_STOP_TIMEOUT)
            {
                warn!("Failed to release PTY {run_id} on owner close: {error}");
            }
        }
        true
    }

    /// Stop one owned run for a non-transferable owner's explicit close. The
    /// caller holds the coordinator run gate after closing owner admission.
    /// `Ok(false)` means nothing of this owner's remained to stop. A failed
    /// kill keeps the entry, backend and flags intact so a retry reaches it.
    pub fn stop_owned_run(&self, run_id: &str, owner_id: &str) -> Result<bool, String> {
        let Ok(entry) = self.entry(run_id) else {
            return Ok(false);
        };
        let live = {
            let state = entry.state.lock();
            if state.owner_id.as_deref() != Some(owner_id) {
                return Ok(false);
            }
            match &state.run {
                RunSlot::Active(live) => Some(Arc::clone(live)),
                RunSlot::Reserved => None,
                RunSlot::Retired => return Ok(false),
            }
        };
        if let Some(live) = &live {
            // Backend I/O happens outside registry and stream locks.
            if let Some(local) = &live.local {
                local.stop(Instant::now() + PROCESS_STOP_TIMEOUT)?;
            } else if !live.event_sink.is_completed() {
                live.backend.kill()?;
            }
            live.fence();
        }
        // Retire only the exact incarnation observed above; a natural exit may
        // already have removed it. A retired reservation cancels its start.
        let mut state = entry.state.lock();
        let mut sessions = self.sessions.lock();
        let same = match (&state.run, &live) {
            (RunSlot::Active(current), Some(expected)) => Arc::ptr_eq(current, expected),
            (RunSlot::Reserved, None) => true,
            // A reservation published a process after the snapshot; stop it on retry.
            (RunSlot::Active(_), None) => {
                return Err(format!("PTY startup completed during stop: {run_id}"));
            }
            _ => false,
        };
        if same
            && sessions
                .get(run_id)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
        {
            state.run = RunSlot::Retired;
            sessions.remove(run_id);
        }
        Ok(true)
    }

    /// The owner of `entry` while it is a live, noncompleted run. Reservations,
    /// completed transcripts and ownerless runs have none.
    fn live_owner(entry: &RunEntry) -> Option<String> {
        let (owner, live) = {
            let state = entry.state.lock();
            match (&state.owner_id, &state.run) {
                (Some(owner), RunSlot::Active(live)) => (owner.clone(), Arc::clone(live)),
                _ => return None,
            }
        };
        (!live.shutdown.load(Ordering::Acquire) && !live.event_sink.is_completed()).then_some(owner)
    }

    fn entries(&self) -> Vec<(String, Arc<RunEntry>)> {
        self.sessions
            .lock()
            .iter()
            .map(|(id, entry)| (id.clone(), Arc::clone(entry)))
            .collect()
    }

    /// Live run ids per owner, each list sorted; see [`Self::live_owner`].
    /// The answer may change right after.
    pub fn live_owner_runs(&self) -> HashMap<String, Vec<String>> {
        let mut runs: HashMap<String, Vec<String>> = HashMap::new();
        for (run_id, entry) in self.entries() {
            if let Some(owner) = Self::live_owner(&entry) {
                runs.entry(owner).or_default().push(run_id);
            }
        }
        runs.values_mut().for_each(|ids| ids.sort());
        runs
    }

    /// Whether `owner_id` has any live run; stops at the first. The answer may
    /// change right after.
    pub fn owner_has_live_run(&self, owner_id: &str) -> bool {
        self.entries()
            .iter()
            .any(|(_, entry)| Self::live_owner(entry).as_deref() == Some(owner_id))
    }

    /// Shut down locals and detach adopted runs. Returns count of local entries.
    pub fn kill_all(&self) -> usize {
        let entries: Vec<_> = self
            .sessions
            .lock()
            .iter()
            .map(|(id, entry)| (id.clone(), Arc::clone(entry)))
            .collect();
        let mut killed = 0;
        for (id, entry) in entries {
            let mut state = entry.state.lock();
            let mut sessions = self.sessions.lock();
            if !sessions
                .get(&id)
                .is_some_and(|current| Arc::ptr_eq(current, &entry))
            {
                continue;
            }
            let run = std::mem::replace(&mut state.run, RunSlot::Retired);
            sessions.remove(&id);
            drop(sessions);
            drop(state);
            match run {
                RunSlot::Active(live) => {
                    if !live.backend.detaches_on_shutdown() {
                        killed += 1;
                    }
                    if let Err(error) =
                        Self::stop_live(&live, false, Instant::now() + PROCESS_STOP_TIMEOUT)
                    {
                        warn!("Failed to kill PTY {id} during shutdown: {error}");
                    }
                }
                RunSlot::Reserved => killed += 1,
                RunSlot::Retired => {}
            }
        }
        killed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn_sh(
        service: &PtyService,
        id: &str,
        script: &str,
        owner: Option<&str>,
        subscriber: Option<PtySubscriber>,
    ) -> Result<PtyEventSink, String> {
        service.spawn(
            id.to_string(),
            &std::env::current_dir().unwrap(),
            "sh",
            &["-c", script],
            None,
            None,
            80,
            24,
            subscriber,
            owner.map(str::to_string),
            None,
            None,
        )
    }

    struct LocalFixture {
        service: PtyService,
        root: PathBuf,
    }

    impl LocalFixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("sworm-pty-drain-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self {
                service: PtyService::new(),
                root: root.canonicalize().unwrap(),
            }
        }

        fn spawn(&self, id: &str, script: &str) {
            self.service
                .spawn(
                    id.into(),
                    &self.root,
                    "sh",
                    &["-c", script],
                    Some(self.root.to_str().unwrap()),
                    None,
                    80,
                    24,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
    }

    impl Drop for LocalFixture {
        fn drop(&mut self) {
            self.service.kill_all();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn wait_for(mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + PROCESS_STOP_TIMEOUT;
        while !ready() {
            assert!(
                Instant::now() < deadline,
                "child never reached expected state"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn explicit_stop_reaps_leader_and_kills_term_ignoring_foreground_group() {
        let fixture = LocalFixture::new();
        fixture.spawn(
            "foreground",
            "set -m; sh -c 'trap \"\" TERM; echo $$ > foreground.pid; exec sleep 60'",
        );
        wait_for(|| {
            std::fs::read_to_string(fixture.root.join("foreground.pid"))
                .is_ok_and(|pid| pid.trim().parse::<i32>().is_ok())
        });
        let foreground: libc::pid_t = std::fs::read_to_string(fixture.root.join("foreground.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let live = fixture.service.active("foreground").unwrap();
        let backend = live.local.as_ref().unwrap();
        let leader = backend.pid.unwrap() as libc::pid_t;
        let foreground_group = unsafe { libc::getpgid(foreground) };
        assert_ne!(foreground_group, unsafe { libc::getpgid(leader) });
        assert_ne!(foreground_group, unsafe { libc::getpgrp() });

        fixture.service.kill("foreground").unwrap();

        assert!(matches!(backend.exit.result(), Some(Ok(_))));
        assert!(groups_terminated(&[foreground_group]).unwrap());
        assert_eq!(
            unsafe { libc::kill(leader, 0) },
            -1,
            "leader was not reaped"
        );
        assert_eq!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ESRCH)
        );
        assert!(fixture.service.pending_exits.lock().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn drain_retains_retired_children_on_timeout_and_acknowledges_fenced_wait() {
        let fixture = LocalFixture::new();
        fixture.spawn("retired", "exec sleep 60");
        let entry = fixture.service.entry("retired").unwrap();
        let live = PtyService::active_entry(&entry, "retired").unwrap();
        let backend = live.local.as_ref().unwrap();
        live.fence();
        PtyService::remove_if_current(&fixture.service.sessions, "retired", &entry);
        assert!(fixture.service.run_state("retired").is_none());

        assert!(fixture
            .service
            .drain_roots(std::slice::from_ref(&fixture.root), Instant::now(),)
            .is_err());
        assert!(!fixture.service.pending_exits.lock().is_empty());
        assert!(backend.exit.result().is_none());
        assert!(fixture.root.is_dir());

        fixture
            .service
            .drain_roots(
                std::slice::from_ref(&fixture.root),
                Instant::now() + PROCESS_STOP_TIMEOUT,
            )
            .unwrap();
        assert!(matches!(backend.exit.result(), Some(Ok(_))));
        // Completion before the waiter subscribes must not lose a wakeup.
        assert!(backend.exit.wait().is_ok());
        assert!(
            !live.event_sink.is_completed(),
            "fenced finalization was not skipped"
        );
        assert!(fixture.service.pending_exits.lock().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn draining_a_retired_incarnation_does_not_stop_reused_id_in_another_project() {
        let unrelated = LocalFixture::new();
        let fixture = LocalFixture::new();
        fixture.spawn("reused", "exec sleep 60");
        let old_entry = fixture.service.entry("reused").unwrap();
        let old = PtyService::active_entry(&old_entry, "reused").unwrap();
        old.fence();
        PtyService::remove_if_current(&fixture.service.sessions, "reused", &old_entry);
        fixture
            .service
            .spawn(
                "reused".into(),
                &unrelated.root,
                "sh",
                &["-c", "exec sleep 60"],
                Some(unrelated.root.to_str().unwrap()),
                None,
                80,
                24,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        let new = fixture.service.active("reused").unwrap();
        let new_backend = new.local.as_ref().unwrap();

        fixture
            .service
            .drain_roots(
                std::slice::from_ref(&fixture.root),
                Instant::now() + PROCESS_STOP_TIMEOUT,
            )
            .unwrap();

        assert!(matches!(
            old.local.as_ref().unwrap().exit.result(),
            Some(Ok(_))
        ));
        assert!(new_backend.exit.result().is_none());
        assert_eq!(fixture.service.run_state("reused"), Some(PtyRunState::Live));
        assert_eq!(unsafe { libc::kill(new_backend.pid.unwrap() as i32, 0) }, 0);
        fixture.service.kill("reused").unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_external_reap_is_a_wait_failure_not_a_successful_drain() {
        let fixture = LocalFixture::new();
        *fixture.service.startup_hook.lock() = Some(Arc::new(|_, pid| {
            if let Some(pid) = pid {
                assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) }, 0);
                let mut status = 0;
                assert_eq!(
                    unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) },
                    pid as i32
                );
            }
        }));
        fixture.spawn("externally-reaped", "exec sleep 60");
        wait_for(|| {
            fixture
                .service
                .pending_exits
                .lock()
                .values()
                .any(|pending| matches!(pending.backend.exit.result(), Some(Err(_))))
        });
        let error = fixture
            .service
            .drain_roots(
                std::slice::from_ref(&fixture.root),
                Instant::now() + PROCESS_STOP_TIMEOUT,
            )
            .unwrap_err();
        assert!(error.contains("Failed waiting for PTY child"));
        assert_eq!(fixture.service.pending_exits.lock().len(), 1);
        assert!(fixture.root.is_dir());
    }

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

    fn channels(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtySubscriber {
        let output_deliveries = Arc::clone(&deliveries);
        let output = Arc::new(move |bytes| {
            output_deliveries.lock().push(Delivery::Output(bytes));
            Ok(())
        });
        let events = Arc::new(move |event: PtyEvent| {
            deliveries.lock().push(Delivery::Event(event_kind(&event)));
            Ok(())
        });
        PtySubscriber { output, events }
    }

    fn channels_rejecting_events(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtySubscriber {
        let output = Arc::new(move |bytes| {
            deliveries.lock().push(Delivery::Output(bytes));
            Ok(())
        });
        let events = Arc::new(|_: PtyEvent| Err("closed".to_string()));
        PtySubscriber { output, events }
    }

    fn sink(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyEventSink {
        PtyEventSink::new(
            "run".to_string(),
            Some(channels(deliveries)),
            RunRetention::Ephemeral,
        )
    }

    fn retained_sink(deliveries: Arc<Mutex<Vec<Delivery>>>) -> PtyEventSink {
        PtyEventSink::new(
            "run".to_string(),
            Some(channels(deliveries)),
            RunRetention::Retained,
        )
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
    struct BlockingBackend {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
        killed: Arc<AtomicBool>,
    }

    impl RunBackend for BlockingBackend {
        fn write(&self, _: &[u8]) -> Result<(), String> {
            self.entered.send(()).unwrap();
            self.release.lock().recv().unwrap();
            Ok(())
        }
        fn resize(&self, _: u16, _: u16) -> Result<(), String> {
            Ok(())
        }
        fn kill(&self) -> Result<(), String> {
            self.killed.store(true, Ordering::Release);
            Ok(())
        }
        fn detaches_on_shutdown(&self) -> bool {
            false
        }
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
        assert!(!sink.complete(Some(0)));

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
        assert!(sink.complete(Some(0)));
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
                Delivery::Event("error"), // The evicted byte is reported before the tail.
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
        assert!(sink.complete(Some(0)));
        sink.resume_original().unwrap();
        assert_eq!(
            *original.lock(),
            vec![Delivery::Event("exit"), Delivery::Event("synced")]
        );
    }

    #[test]
    fn retained_mode_retains_while_active() {
        let active = Arc::new(Mutex::new(Vec::new()));
        let sink = retained_sink(active);
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
    fn retained_mode_cursor_inside_coalesced_chunk() {
        let sink = retained_sink(Arc::new(Mutex::new(Vec::new())));
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
    fn retained_transfer_slices_coalesced_boundary_and_restores_source_independently() {
        let source = Arc::new(Mutex::new(Vec::new()));
        let sink = retained_sink(Arc::clone(&source));
        sink.emit_output(b"before".to_vec());
        assert_eq!(sink.pause(), 1); // Render barrier, not the output byte cursor.
        sink.emit_output(b"after".to_vec()); // Coalesces into one retained chunk.

        let target = Arc::new(Mutex::new(Vec::new()));
        sink.attach(channels(Arc::clone(&target))).unwrap();
        sink.emit(error_event());
        sink.emit_output(b"more".to_vec());
        assert_eq!(
            *target.lock(),
            vec![
                Delivery::Output(b"after".to_vec()),
                Delivery::Event("synced"),
                Delivery::Event("error"),
                Delivery::Output(b"more".to_vec()),
            ]
        );
        assert_eq!(sink.resume_original().unwrap(), 4);
        assert_eq!(
            *source.lock(),
            vec![
                Delivery::Output(b"before".to_vec()),
                Delivery::Output(b"after".to_vec()),
                Delivery::Event("error"),
                Delivery::Output(b"more".to_vec()),
                Delivery::Event("synced"),
            ]
        );
        sink.emit_output(b"live".to_vec());
        assert_eq!(
            source.lock().last(),
            Some(&Delivery::Output(b"live".to_vec()))
        );
    }

    #[test]
    fn retained_transfer_retry_resumes_target_cursor_after_partial_delivery() {
        let sink = retained_sink(Arc::new(Mutex::new(Vec::new())));
        sink.emit_output(b"old".to_vec());
        sink.pause();
        sink.emit_output(b"new".to_vec());
        sink.emit(error_event());
        let failed = Arc::new(Mutex::new(Vec::new()));
        assert!(sink
            .attach(channels_rejecting_events(Arc::clone(&failed)))
            .is_err());
        assert_eq!(*failed.lock(), vec![Delivery::Output(b"new".to_vec())]);
        let retry = Arc::new(Mutex::new(Vec::new()));
        sink.attach(channels(Arc::clone(&retry))).unwrap();
        assert_eq!(
            *retry.lock(),
            vec![Delivery::Event("error"), Delivery::Event("synced")]
        );
        let replay = Arc::new(Mutex::new(Vec::new()));
        sink.commit_transfer();
        sink.attach_from(replay_sink(Arc::clone(&replay)), PtyCursor::default())
            .unwrap();
        assert_eq!(
            *replay.lock(),
            vec![
                ReplayDelivery::Output(0, b"oldnew".to_vec()),
                ReplayDelivery::Event(1, "error"),
                ReplayDelivery::Event(1, "synced"),
            ]
        );
    }

    #[test]
    fn retained_transfer_completion_replays_after_commit_and_abort() {
        for commit in [true, false] {
            let source = Arc::new(Mutex::new(Vec::new()));
            let sink = retained_sink(Arc::clone(&source));
            sink.emit(PtyEvent::Started {
                run_id: "run".to_string(),
                pid: Some(123),
            });
            sink.pause();
            sink.emit_output(b"tail".to_vec());
            let target = Arc::new(Mutex::new(Vec::new()));
            sink.attach(channels(Arc::clone(&target))).unwrap();
            assert!(sink.complete(Some(0)));
            if commit {
                sink.commit_transfer();
            } else {
                sink.resume_original().unwrap();
                assert_eq!(
                    *source.lock(),
                    vec![
                        Delivery::Event("started"),
                        Delivery::Output(b"tail".to_vec()),
                        Delivery::Event("exit"),
                        Delivery::Event("synced"),
                    ]
                );
            }
            let replay = Arc::new(Mutex::new(Vec::new()));
            sink.attach_from(replay_sink(Arc::clone(&replay)), PtyCursor::default())
                .unwrap();
            assert_eq!(
                *replay.lock(),
                vec![
                    ReplayDelivery::Event(1, "started"),
                    ReplayDelivery::Output(0, b"tail".to_vec()),
                    ReplayDelivery::Event(2, "exit"),
                    ReplayDelivery::Event(2, "synced"),
                ]
            );
        }
    }

    #[test]
    fn failed_source_rollback_preserves_completed_retained_transcript() {
        let source = Arc::new(Mutex::new(Vec::new()));
        let sink = PtyEventSink::new(
            "run".to_string(),
            Some(channels_rejecting_events(Arc::clone(&source))),
            RunRetention::Retained,
        );
        sink.pause();
        sink.emit_output(b"tail".to_vec());
        sink.complete(Some(0));
        assert!(sink.resume_original().is_err());
        sink.abort_transfer_detached().unwrap();
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
    fn retained_native_transfer_reports_evicted_output_before_replaying_tail() {
        let sink = PtyEventSink::new("run".to_string(), None, RunRetention::Retained);
        sink.pause();
        sink.emit_output(vec![7; RETAINED_OUTPUT_CAP + 4]);
        let target = Arc::new(Mutex::new(Vec::new()));
        sink.attach(channels(Arc::clone(&target))).unwrap();
        let deliveries = target.lock();
        assert_eq!(deliveries.first(), Some(&Delivery::Event("error")));
        assert_eq!(
            deliveries.get(1),
            Some(&Delivery::Output(vec![7; RETAINED_OUTPUT_CAP]))
        );
        assert_eq!(deliveries.get(2), Some(&Delivery::Event("synced")));
    }

    #[test]
    fn retained_mode_events_replay_by_sequence() {
        let sink = retained_sink(Arc::new(Mutex::new(Vec::new())));
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
    fn detached_retained_lifecycle_event_replays_without_subscriber() {
        let sink = PtyEventSink::new("run".to_string(), None, RunRetention::Retained);
        sink.emit(PtyEvent::ResumeTokenBound {
            run_id: "run".to_string(),
            token: "provider-token".to_string(),
        });
        let replay = Arc::new(Mutex::new(Vec::new()));
        sink.attach_from(replay_sink(Arc::clone(&replay)), PtyCursor::default())
            .unwrap();
        assert_eq!(
            *replay.lock(),
            vec![
                ReplayDelivery::Event(1, "resumeTokenBound"),
                ReplayDelivery::Event(1, "synced"),
            ]
        );
    }

    #[test]
    fn retained_mode_gap_when_trimmed_past_cursor() {
        let sink = retained_sink(Arc::new(Mutex::new(Vec::new())));
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
    fn completed_retained_run_replays_tail_before_exit() {
        let sink = retained_sink(Arc::new(Mutex::new(Vec::new())));
        sink.emit_output(b"tail".to_vec());
        assert!(sink.complete(Some(0)));

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
    fn retained_replay_does_not_block_writes_to_other_runs() {
        let service = PtyService::new();
        let replay = spawn_sh(&service, "replay", "sleep 30", Some("owner"), None).unwrap();
        replay.emit_output(b"retained".to_vec());
        service
            .adopt(
                "other".into(),
                Box::new(FakeBackend {
                    killed: Arc::new(AtomicBool::new(false)),
                    detaches: true,
                }),
                None,
                Some("other".into()),
            )
            .unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let (write, attached) = std::thread::scope(|scope| {
            let service = &service;
            let attach = scope.spawn(move || {
                service.attach_same_owner(
                    "replay",
                    Some("owner"),
                    PtySubscriber {
                        output: Arc::new(move |bytes| {
                            assert_eq!(bytes, b"retained");
                            entered_tx.send(()).unwrap();
                            release_rx
                                .lock()
                                .recv_timeout(std::time::Duration::from_secs(5))
                                .unwrap();
                            Ok(())
                        }),
                        events: Arc::new(|_| Ok(())),
                    },
                    80,
                    24,
                )
            });
            entered_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            let (write_tx, write_rx) = std::sync::mpsc::channel();
            scope.spawn(move || {
                let result = (|| {
                    service.write("other", b"input")?;
                    let owned = service.owner_run_ids("other", &HashSet::new());
                    if owned != vec!["other".to_string()] {
                        return Err(format!("unexpected owner runs: {owned:?}"));
                    }
                    if !service.kill_owned_run("other", "other", &HashSet::new()) {
                        return Err("owner cleanup did not retire run".to_string());
                    }
                    Ok::<_, String>(())
                })();
                write_tx.send(result).unwrap();
            });
            let write = write_rx.recv_timeout(std::time::Duration::from_secs(2));
            release_tx.send(()).unwrap();
            (write, attach.join().unwrap())
        });
        service.kill_all();
        attached.unwrap();
        write
            .expect("unrelated owner cleanup blocked by replay")
            .unwrap();
    }

    #[test]
    fn adopted_run_kill_all_detaches() {
        let service = PtyService::new();
        let adopted_killed = Arc::new(AtomicBool::new(false));
        let (output, events) = noop_channels();
        service
            .adopt(
                "adopted".to_string(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&adopted_killed),
                    detaches: true,
                }),
                Some(PtySubscriber { output, events }),
                None,
            )
            .unwrap();

        let (output, events) = noop_channels();
        spawn_sh(
            &service,
            "local",
            "sleep 60",
            None,
            Some(PtySubscriber { output, events }),
        )
        .unwrap();

        assert_eq!(service.kill_all(), 1);
        assert!(!adopted_killed.load(Ordering::Acquire));
    }

    #[test]
    fn adopted_run_kill_owned_run_kills() {
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
                Some(PtySubscriber { output, events }),
                Some("owner".to_string()),
            )
            .unwrap();

        assert!(service.kill_owned_run("adopted", "owner", &HashSet::new()));
        assert!(killed.load(Ordering::Acquire));
    }

    #[test]
    fn live_owner_runs_only_live_owned_runs() {
        let service = PtyService::new();
        let adopt = |id: &str, owner: Option<&str>, detaches: bool| {
            service
                .adopt(
                    id.to_string(),
                    Box::new(FakeBackend {
                        killed: Arc::new(AtomicBool::new(false)),
                        detaches,
                    }),
                    None,
                    owner.map(str::to_string),
                )
                .unwrap();
        };
        adopt("a1", Some("a"), false);
        adopt("a2", Some("a"), false);
        adopt("b-done", Some("b"), true);
        adopt("ownerless", None, false);
        service.reserve("a-starting", Some("a".into())).unwrap();
        service.complete_adopted("b-done", Some(0)).unwrap();
        adopt("c-retained", Some("c"), false);
        service
            .active("c-retained")
            .unwrap()
            .event_sink
            .state
            .lock()
            .exit_code = Some(None);

        assert_eq!(
            service.live_owner_runs(),
            HashMap::from([("a".to_string(), vec!["a1".to_string(), "a2".to_string()])])
        );
        assert!(service.owner_has_live_run("a"));
        for owner in ["b", "c", "missing"] {
            assert!(!service.owner_has_live_run(owner), "{owner}");
        }
    }

    #[test]
    fn stop_owned_run_ignores_foreign_and_retires_reservation_and_completion() {
        let service = PtyService::new();
        let killed = Arc::new(AtomicBool::new(false));
        service
            .adopt(
                "foreign".to_string(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&killed),
                    detaches: false,
                }),
                None,
                Some("other".to_string()),
            )
            .unwrap();
        assert_eq!(service.stop_owned_run("foreign", "owner"), Ok(false));
        assert!(!killed.load(Ordering::Acquire));
        assert_eq!(service.run_state("foreign"), Some(PtyRunState::Live));
        assert_eq!(service.stop_owned_run("missing", "owner"), Ok(false));

        service.reserve("starting", Some("owner".into())).unwrap();
        assert_eq!(service.stop_owned_run("starting", "owner"), Ok(true));
        assert!(service.entry("starting").is_err());

        let done_killed = Arc::new(AtomicBool::new(false));
        service
            .adopt(
                "done".to_string(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&done_killed),
                    detaches: false,
                }),
                None,
                Some("owner".to_string()),
            )
            .unwrap();
        service
            .active("done")
            .unwrap()
            .event_sink
            .state
            .lock()
            .exit_code = Some(None);
        assert_eq!(service.stop_owned_run("done", "owner"), Ok(true));
        assert!(
            !done_killed.load(Ordering::Acquire),
            "completed run killed again"
        );
        assert!(service.run_state("done").is_none());
    }

    #[test]
    fn detach_owned_run_detaches_adopted_and_kills_local() {
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
                    Some(PtySubscriber { output, events }),
                    Some("owner".to_string()),
                )
                .unwrap();
        }

        for id in ["adopted", "local"] {
            assert!(service.detach_owned_run(id, "owner", &HashSet::new()));
        }
        assert!(!adopted_killed.load(Ordering::Acquire));
        assert!(local_killed.load(Ordering::Acquire));
        assert_eq!(service.run_state("adopted"), None);
        assert_eq!(service.run_state("local"), None);
    }

    #[test]
    fn adopted_proxy_detach_respects_owner_and_transfer_without_stopping_backend() {
        let service = PtyService::new();
        let killed = Arc::new(AtomicBool::new(false));
        let old_view = Arc::new(Mutex::new(Vec::new()));
        let sink = service
            .adopt(
                "proxy".to_string(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&killed),
                    detaches: true,
                }),
                Some(channels(Arc::clone(&old_view))),
                Some("owner".to_string()),
            )
            .unwrap();
        assert!(service.detach_adopted("proxy", Some("other")).is_err());
        service.pause_owned("proxy", "owner").unwrap();
        assert!(service.detach_adopted("proxy", Some("owner")).is_err());
        service.commit_transfer("proxy");
        service.detach_adopted("proxy", Some("owner")).unwrap();
        sink.emit_output(b"late".to_vec());
        assert!(old_view.lock().is_empty());
        assert_eq!(service.run_state("proxy"), None);
        assert!(!killed.load(Ordering::Acquire));
    }

    #[test]
    fn adopted_owner_reassignment_protects_target_from_source_cleanup() {
        let service = PtyService::new();
        let killed = Arc::new(AtomicBool::new(false));
        service
            .adopt(
                "moved-proxy".into(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&killed),
                    detaches: true,
                }),
                Some(channels(Arc::new(Mutex::new(Vec::new())))),
                Some("source".into()),
            )
            .unwrap();
        assert!(service
            .reassign_adopted("moved-proxy", "other", "target")
            .is_err());
        service
            .reassign_adopted("moved-proxy", "source", "target")
            .unwrap();
        assert!(service
            .detach_adopted("moved-proxy", Some("source"))
            .is_err());
        assert!(!service.stop_owned_run("moved-proxy", "source").unwrap());
        service.ensure_owner("moved-proxy", Some("target")).unwrap();
        service
            .reassign_adopted("moved-proxy", "target", "source")
            .unwrap();
        service.pause_owned("moved-proxy", "source").unwrap();
        assert!(service
            .reassign_adopted("moved-proxy", "source", "target")
            .is_err());
        service.commit_transfer("moved-proxy");
        service
            .detach_adopted("moved-proxy", Some("source"))
            .unwrap();
        assert!(!killed.load(Ordering::Acquire));
    }

    #[test]
    fn stale_attachment_cannot_pause_replacement() {
        let sink = retained_sink(Arc::new(Mutex::new(Vec::new())));
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

    #[test]
    fn concurrent_duplicate_spawn_never_replaces_process() {
        let service = Arc::new(PtyService::new());
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let attempts = (0..2)
            .map(|_| {
                let service = Arc::clone(&service);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    spawn_sh(&service, "duplicate", "sleep 60", None, None)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = attempts
            .into_iter()
            .map(|attempt| attempt.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(service.run_state("duplicate"), Some(PtyRunState::Live));
        service.kill("duplicate").unwrap();
    }

    #[test]
    fn detached_retained_run_replays_started_tail_exit_without_respawning() {
        let service = PtyService::new();
        spawn_sh(&service, "detached", "printf tail", Some("owner"), None).unwrap();
        for _ in 0..200 {
            if service.run_state("detached") == Some(PtyRunState::Completed(Some(0))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            service.run_state("detached"),
            Some(PtyRunState::Completed(Some(0)))
        );
        let deliveries = Arc::new(Mutex::new(Vec::new()));
        assert!(service.ensure_owner("detached", Some("other")).is_err());
        assert_eq!(
            service
                .attach_same_owner(
                    "detached",
                    Some("owner"),
                    channels(Arc::clone(&deliveries)),
                    80,
                    24,
                )
                .unwrap(),
            PtyRunState::Completed(Some(0))
        );
        assert_eq!(
            *deliveries.lock(),
            vec![
                Delivery::Event("started"),
                Delivery::Output(b"tail".to_vec()),
                Delivery::Event("exit"),
                Delivery::Event("synced"),
            ]
        );
        service.kill("detached").unwrap();
    }

    #[test]
    fn retained_completion_publishes_outside_pty_locks() {
        let service = Arc::new(PtyService::new());
        let transcript = Arc::new(Mutex::new(None));
        let callback_service = Arc::clone(&service);
        let callback_transcript = Arc::clone(&transcript);
        let archive: CompletedRunSink = Arc::new(move |run| {
            assert_eq!(
                callback_service.run_state("archived"),
                Some(PtyRunState::Completed(Some(0)))
            );
            assert!(callback_service.cursor("archived").is_some());
            *callback_transcript.lock() = Some(run);
        });
        service
            .spawn(
                "archived".to_string(),
                &std::env::current_dir().unwrap(),
                "sh",
                &["-c", "printf archived"],
                None,
                None,
                80,
                24,
                None,
                None,
                None,
                Some(archive),
            )
            .unwrap();
        for _ in 0..200 {
            if transcript.lock().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let run = transcript
            .lock()
            .take()
            .expect("completed transcript published");
        assert_eq!(run.output, b"archived");
        assert!(matches!(
            run.events.first(),
            Some((_, PtyEvent::Started { .. }))
        ));
        assert!(matches!(
            run.events.last(),
            Some((_, PtyEvent::Exit { .. }))
        ));
    }

    #[test]
    fn pending_transfer_rejects_ordinary_reattach() {
        let service = PtyService::new();
        let original = Arc::new(Mutex::new(Vec::new()));
        let sink = spawn_sh(
            &service,
            "transfer",
            "sleep 60",
            Some("owner"),
            Some(channels(Arc::clone(&original))),
        )
        .unwrap();
        assert!(service
            .attach_same_owner(
                "transfer",
                Some("other"),
                channels(Arc::new(Mutex::new(Vec::new()))),
                80,
                24,
            )
            .is_err());
        sink.emit_output(b"still owned".to_vec());
        assert!(original
            .lock()
            .contains(&Delivery::Output(b"still owned".to_vec())));
        service.pause_owned("transfer", "owner").unwrap();
        assert!(service.ensure_owner("transfer", Some("owner")).is_err());
        assert!(service
            .attach_same_owner(
                "transfer",
                Some("owner"),
                channels(Arc::new(Mutex::new(Vec::new()))),
                80,
                24,
            )
            .is_err());
        service.kill("transfer").unwrap();
    }
    #[test]
    fn completed_local_transfer_keeps_run_for_new_owner_replay() {
        let service = PtyService::new();
        let source = Arc::new(Mutex::new(Vec::new()));
        spawn_sh(
            &service,
            "completed-transfer",
            "sleep 0.1; printf tail",
            Some("source"),
            Some(channels(Arc::clone(&source))),
        )
        .unwrap();
        service.pause_owned("completed-transfer", "source").unwrap();
        assert!(service
            .ensure_transfer_owner("completed-transfer", "other")
            .is_err());
        service
            .ensure_transfer_owner("completed-transfer", "source")
            .unwrap();
        for _ in 0..200 {
            if service.run_state("completed-transfer") == Some(PtyRunState::Completed(Some(0))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            service.run_state("completed-transfer"),
            Some(PtyRunState::Completed(Some(0)))
        );
        let target = Arc::new(Mutex::new(Vec::new()));
        service
            .attach("completed-transfer", channels(Arc::clone(&target)))
            .unwrap();
        assert!(target.lock().contains(&Delivery::Output(b"tail".to_vec())));
        service
            .transfer_owner("completed-transfer", "target")
            .unwrap();
        service.commit_transfer("completed-transfer");
        assert_eq!(
            service.run_state("completed-transfer"),
            Some(PtyRunState::Completed(Some(0)))
        );
        let reloaded = Arc::new(Mutex::new(Vec::new()));
        service
            .attach_same_owner(
                "completed-transfer",
                Some("target"),
                channels(Arc::clone(&reloaded)),
                80,
                24,
            )
            .unwrap();
        assert_eq!(
            *reloaded.lock(),
            vec![
                Delivery::Event("started"),
                Delivery::Output(b"tail".to_vec()),
                Delivery::Event("exit"),
                Delivery::Event("synced"),
            ]
        );
        service.kill("completed-transfer").unwrap();
    }
    #[test]
    fn reserved_start_does_not_block_other_real_pty() {
        let service = Arc::new(PtyService::new());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        *service.startup_hook.lock() = Some(Arc::new(move |id, pid| {
            if id == "held" && pid.is_none() {
                entered_tx.send(()).unwrap();
                release_rx.lock().recv().unwrap();
            }
        }));
        std::thread::scope(|scope| {
            let held = scope.spawn(|| spawn_sh(&service, "held", "sleep 60", Some("owner"), None));
            if entered_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                let _ = release_tx.send(());
                panic!("reserved startup did not reach barrier");
            }
            assert!(spawn_sh(&service, "held", "exit 0", Some("owner"), None).is_err());
            let (progress_tx, progress_rx) = std::sync::mpsc::channel();
            let service = &service;
            scope.spawn(move || {
                let result = (|| {
                    spawn_sh(service, "other", "sleep 60", None, None)?;
                    let operations = (|| {
                        service.write("other", b"echo independent\n")?;
                        service.resize("other", 100, 30)
                    })();
                    let stopped = service.kill("other");
                    operations?;
                    stopped
                })();
                progress_tx.send(result).unwrap();
            });
            let progress = progress_rx.recv_timeout(Duration::from_secs(3));
            if progress.is_err() {
                release_tx.send(()).unwrap();
                if held.join().unwrap().is_ok() {
                    service.kill("held").unwrap();
                }
                panic!("reserved startup blocked unrelated PTY");
            }
            service.kill("held").unwrap();
            release_tx.send(()).unwrap();
            assert!(held.join().unwrap().is_err());
            progress
                .expect("reserved startup blocked unrelated PTY")
                .unwrap();
        });
        assert_eq!(service.run_state("held"), None);
    }

    #[test]
    fn reserved_child_is_reaped_when_stopped_by_id_owner_or_shutdown() {
        for mode in 0..3 {
            let service = Arc::new(PtyService::new());
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let release_rx = Mutex::new(release_rx);
            *service.startup_hook.lock() = Some(Arc::new(move |id, pid| {
                if id == "held" {
                    if let Some(pid) = pid {
                        entered_tx.send(pid).unwrap();
                        release_rx.lock().recv().unwrap();
                    }
                }
            }));
            std::thread::scope(|scope| {
                let held =
                    scope.spawn(|| spawn_sh(&service, "held", "sleep 60", Some("owner"), None));
                let pid = entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(pid > 0);
                match mode {
                    0 => service.kill("held").unwrap(),
                    1 => assert!(service.kill_owned_run("held", "owner", &HashSet::new())),
                    _ => assert_eq!(service.kill_all(), 1),
                }
                release_tx.send(()).unwrap();
                assert!(held.join().unwrap().is_err());
                assert_eq!(service.run_state("held"), None);
                assert_eq!(
                    unsafe { libc::kill(pid as i32, 0) },
                    -1,
                    "cancelled child survived"
                );
                assert_eq!(
                    std::io::Error::last_os_error().raw_os_error(),
                    Some(libc::ESRCH)
                );
            });
        }
    }

    #[test]
    fn blocked_backend_write_does_not_hold_registry_or_fence_kill() {
        let service = Arc::new(PtyService::new());
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let killed = Arc::new(AtomicBool::new(false));
        service
            .adopt(
                "blocked".into(),
                Box::new(BlockingBackend {
                    entered: entered_tx,
                    release: Mutex::new(release_rx),
                    killed: Arc::clone(&killed),
                }),
                None,
                None,
            )
            .unwrap();
        service
            .adopt(
                "other".into(),
                Box::new(FakeBackend {
                    killed: Arc::new(AtomicBool::new(false)),
                    detaches: false,
                }),
                None,
                None,
            )
            .unwrap();
        std::thread::scope(|scope| {
            let writer = scope.spawn(|| service.write("blocked", b"data"));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (progress_tx, progress_rx) = std::sync::mpsc::channel();
            let service = &service;
            scope.spawn(move || {
                progress_tx
                    .send((|| {
                        service.write("other", b"independent")?;
                        service.resize("other", 80, 24)?;
                        service.kill("blocked")
                    })())
                    .unwrap();
            });
            let progress = progress_rx.recv_timeout(Duration::from_secs(3));
            release_tx.send(()).unwrap();
            writer.join().unwrap().unwrap();
            progress
                .expect("blocked write prevented unrelated I/O or kill")
                .unwrap();
        });
        assert!(killed.load(Ordering::Acquire));
        service.kill("other").unwrap();
    }
    #[test]
    fn stale_stream_cannot_pause_new_incarnation_with_same_run_id() {
        let service = PtyService::new();
        spawn_sh(&service, "same", "sleep 60", None, None).unwrap();
        let old = service
            .attach_from(
                "same",
                replay_sink(Arc::new(Mutex::new(Vec::new()))),
                PtyCursor::default(),
            )
            .unwrap();
        service.kill("same").unwrap();
        spawn_sh(&service, "same", "sleep 60", None, None).unwrap();
        let current = service
            .attach_from(
                "same",
                replay_sink(Arc::new(Mutex::new(Vec::new()))),
                PtyCursor::default(),
            )
            .unwrap();
        assert_ne!(old, current);
        assert!(!service.pause_attachment("same", old).unwrap());
        assert!(service.pause_attachment("same", current).unwrap());
        service.kill("same").unwrap();
    }
    #[test]
    fn owner_cleanup_cannot_retire_pending_or_transferred_run() {
        let service = PtyService::new();
        let killed = Arc::new(AtomicBool::new(false));
        service
            .adopt(
                "moved".into(),
                Box::new(FakeBackend {
                    killed: Arc::clone(&killed),
                    detaches: true,
                }),
                Some(channels(Arc::new(Mutex::new(Vec::new())))),
                Some("source".into()),
            )
            .unwrap();
        service.pause_owned("moved", "source").unwrap();
        assert!(!service.kill_owned_run("moved", "source", &HashSet::new()));
        service.transfer_owner("moved", "target").unwrap();
        assert!(!service.kill_owned_run("moved", "source", &HashSet::new()));
        service.commit_transfer("moved");
        assert!(service.kill_owned_run("moved", "target", &HashSet::new()));
        assert!(killed.load(Ordering::Acquire));
    }
}
