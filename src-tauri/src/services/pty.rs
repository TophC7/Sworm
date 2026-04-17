use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

/// Callback invoked whenever raw PTY bytes are read from the child.
///
/// The transcript sink is responsible for durable persistence to disk.
/// It is called on the reader thread, so implementations must be cheap
/// and non-blocking (coalesce + batch writes, do not touch the DB on
/// every chunk).
pub type TranscriptSink = Box<dyn Fn(&str, &[u8]) + Send + Sync>;

/// Callback invoked right before a live PTY is killed to make way for
/// a respawn on the same session_id. Gives the caller a chance to drain
/// any in-memory batches for the outgoing runtime so its tail bytes are
/// persisted cleanly and never mixed with the replacement runtime's
/// output.
pub type RespawnCleanup = Box<dyn Fn(&str) + Send + Sync>;

/// Events emitted over the lifecycle channel.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PtyEvent {
    Started {
        session_id: String,
        pid: Option<u32>,
    },
    Exit {
        session_id: String,
        code: Option<i32>,
    },
    Error {
        session_id: String,
        message: String,
    },
}

/// A single live PTY session.
struct LivePty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    shutdown: Arc<AtomicBool>,
    finalized: Arc<AtomicBool>,
    runtime_id: String,
}

/// PTY service managing all active pseudo-terminal sessions.
pub struct PtyService {
    sessions: Arc<Mutex<HashMap<String, LivePty>>>,
    transcript_sink: Arc<Mutex<Option<Arc<TranscriptSink>>>>,
    respawn_cleanup: Arc<Mutex<Option<Arc<RespawnCleanup>>>>,
}

impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            transcript_sink: Arc::new(Mutex::new(None)),
            respawn_cleanup: Arc::new(Mutex::new(None)),
        }
    }

    /// Install the transcript sink. Called once at startup from AppState.
    pub fn set_transcript_sink(&self, sink: TranscriptSink) {
        *self.transcript_sink.lock() = Some(Arc::new(sink));
    }

    /// Install the respawn cleanup callback. Called once at startup.
    pub fn set_respawn_cleanup(&self, cleanup: RespawnCleanup) {
        *self.respawn_cleanup.lock() = Some(Arc::new(cleanup));
    }

    /// Spawn a new PTY running the given command.
    ///
    /// If a PTY already exists for this session_id, it is killed first.
    pub fn spawn(
        &self,
        session_id: String,
        cmd: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: Option<&HashMap<String, String>>,
        cols: u16,
        rows: u16,
        output_channel: tauri::ipc::Channel<Vec<u8>>,
        event_channel: tauri::ipc::Channel<PtyEvent>,
        on_exit: Option<Box<dyn FnOnce(&str, Option<i32>) + Send>>,
    ) -> Result<(), String> {
        if self.is_alive(&session_id) {
            info!("Killing existing PTY for {} before respawn", session_id);
            // Let the outgoing runtime's tail bytes drain to disk before
            // we kill it — otherwise up to ~16 KiB of the previous run's
            // output ends up either dropped or mixed with the new run's
            // bytes under the same session_id.
            if let Some(cleanup) = self.respawn_cleanup.lock().clone() {
                cleanup(&session_id);
            }
            let _ = self.kill(&session_id);
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
        let detached = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));
        let runtime_id = uuid::Uuid::new_v4().to_string();

        self.sessions.lock().insert(
            session_id.clone(),
            LivePty {
                master: pair.master,
                writer,
                killer,
                shutdown: shutdown.clone(),
                finalized: finalized.clone(),
                runtime_id: runtime_id.clone(),
            },
        );

        let _ = event_channel.send(PtyEvent::Started {
            session_id: session_id.clone(),
            pid,
        });

        let sid_for_thread = session_id.clone();
        let sessions_for_thread = Arc::clone(&self.sessions);
        let sink_for_thread = Arc::clone(&self.transcript_sink);

        std::thread::Builder::new()
            .name(format!("pty-reader-{}", &session_id))
            .spawn(move || {
                let mut buf = [0u8; 4096];
                let sink = sink_for_thread.lock().clone();

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
                        Ok(n) => {
                            let chunk = &buf[..n];

                            // Always hand bytes to the transcript sink,
                            // even if the frontend channel is gone —
                            // that's what lets sessions survive webview
                            // reloads and produce durable transcripts.
                            if let Some(sink) = sink.as_ref() {
                                sink(&sid_for_thread, chunk);
                            }

                            if !detached.load(Ordering::Relaxed) {
                                if output_channel.send(chunk.to_vec()).is_err() {
                                    detached.store(true, Ordering::Relaxed);
                                    info!(
                                        "Output channel closed for {}, continuing to drain",
                                        sid_for_thread
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            if shutdown.load(Ordering::Relaxed) {
                                info!(
                                    "PTY read loop stopped during shutdown for {}",
                                    sid_for_thread
                                );
                            } else {
                                error!("PTY read error for {}: {}", sid_for_thread, err);
                                let _ = event_channel.send(PtyEvent::Error {
                                    session_id: sid_for_thread.clone(),
                                    message: err.to_string(),
                                });
                            }
                            break;
                        }
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

                {
                    let mut sessions = sessions_for_thread.lock();
                    let remove_current = sessions
                        .get(&sid_for_thread)
                        .map(|live| live.runtime_id == runtime_id)
                        .unwrap_or(false);
                    if remove_current {
                        sessions.remove(&sid_for_thread);
                    }
                }

                // CAS-style guard: only one caller runs the finalization path.
                // Prevents duplicate exit callbacks if kill() races with EOF.
                if finalized.swap(true, Ordering::AcqRel) {
                    info!("Skipping duplicate PTY finalization for {}", sid_for_thread);
                    return;
                }

                if detached.load(Ordering::Relaxed) {
                    info!(
                        "PTY {} continued after frontend detach and exited with {:?}",
                        sid_for_thread, exit_code
                    );
                }

                if let Some(callback) = on_exit {
                    callback(&sid_for_thread, exit_code);
                }

                let _ = event_channel.send(PtyEvent::Exit {
                    session_id: sid_for_thread,
                    code: exit_code,
                });
            })
            .map_err(|e| format!("Failed to spawn reader thread: {}", e))?;

        Ok(())
    }

    /// Write data to the PTY's stdin.
    pub fn write(&self, session_id: &str, data: &[u8]) -> Result<(), String> {
        let mut sessions = self.sessions.lock();
        let live = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("No active PTY session: {}", session_id))?;

        live.writer
            .write_all(data)
            .map_err(|e| format!("PTY write failed: {}", e))?;
        live.writer
            .flush()
            .map_err(|e| format!("PTY flush failed: {}", e))?;
        Ok(())
    }

    /// Resize the PTY.
    pub fn resize(&self, session_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let sessions = self.sessions.lock();
        let live = sessions
            .get(session_id)
            .ok_or_else(|| format!("No active PTY session: {}", session_id))?;

        live.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("PTY resize failed: {}", e))?;

        Ok(())
    }

    /// Kill the PTY process and clean up.
    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let live = self.sessions.lock().remove(session_id);
        if let Some(mut live) = live {
            live.shutdown.store(true, Ordering::Relaxed);
            // Pre-mark finalized so the reader thread won't double-fire
            // the exit callback after we've initiated shutdown.
            live.finalized.store(true, Ordering::Release);

            live.killer
                .kill()
                .map_err(|e| format!("Failed to kill PTY child: {}", e))?;

            info!("PTY session {} killed", session_id);
            Ok(())
        } else {
            Err(format!("No active PTY session: {}", session_id))
        }
    }

    /// Kill all PTYs currently tracked by the service.
    pub fn kill_all(&self) -> usize {
        let live_sessions: Vec<(String, LivePty)> = self.sessions.lock().drain().collect();
        let total = live_sessions.len();

        for (session_id, mut live) in live_sessions {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);

            if let Err(err) = live.killer.kill() {
                warn!(
                    "Failed to kill PTY child {} during shutdown: {}",
                    session_id, err
                );
            } else {
                info!("Cleanup: killed PTY {}", session_id);
            }
        }

        total
    }

    /// Check if a session is alive.
    pub fn is_alive(&self, session_id: &str) -> bool {
        self.sessions.lock().contains_key(session_id)
    }

    /// Kill all PTYs whose session_id is in the given list.
    #[allow(dead_code)]
    pub fn kill_many(&self, session_ids: &[String]) -> usize {
        let mut killed = 0;
        for session_id in session_ids {
            if self.kill(session_id).is_ok() {
                killed += 1;
            }
        }
        killed
    }
}

/// Debounced, append-only writer that coalesces PTY bytes before sending
/// them to the transcript persistence callback.
///
/// Designed to be called from the PTY reader thread on every chunk.
/// It batches bytes per session and flushes on either:
///   - accumulated buffer reaches `flush_bytes`, or
///   - `flush_interval` elapsed since the last flush.
///
/// Flushes are performed synchronously from whichever caller trips the
/// threshold, so the caller absorbs one write per batch — not per chunk.
pub struct TranscriptBatcher {
    inner: Arc<Mutex<HashMap<String, BatchedSession>>>,
    flush_bytes: usize,
    flush_interval: Duration,
    on_flush: Arc<dyn Fn(&str, &[u8]) + Send + Sync>,
}

struct BatchedSession {
    buf: Vec<u8>,
    last_flush: Instant,
}

impl TranscriptBatcher {
    pub fn new<F>(flush_bytes: usize, flush_interval: Duration, on_flush: F) -> Self
    where
        F: Fn(&str, &[u8]) + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            flush_bytes,
            flush_interval,
            on_flush: Arc::new(on_flush),
        }
    }

    /// Append bytes for `session_id`. May trigger a synchronous flush.
    pub fn push(&self, session_id: &str, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }

        let to_flush: Option<Vec<u8>> = {
            let mut map = self.inner.lock();
            let entry = map
                .entry(session_id.to_string())
                .or_insert_with(|| BatchedSession {
                    buf: Vec::with_capacity(self.flush_bytes),
                    last_flush: Instant::now(),
                });
            entry.buf.extend_from_slice(chunk);

            let should_flush = entry.buf.len() >= self.flush_bytes
                || entry.last_flush.elapsed() >= self.flush_interval;
            if should_flush {
                let taken = std::mem::take(&mut entry.buf);
                entry.last_flush = Instant::now();
                Some(taken)
            } else {
                None
            }
        };

        if let Some(bytes) = to_flush {
            (self.on_flush)(session_id, &bytes);
        }
    }

    /// Force-flush any pending bytes for `session_id`. Used at session
    /// stop, PTY exit, or app shutdown.
    pub fn flush(&self, session_id: &str) {
        let bytes: Option<Vec<u8>> = {
            let mut map = self.inner.lock();
            map.get_mut(session_id).and_then(|entry| {
                if entry.buf.is_empty() {
                    None
                } else {
                    entry.last_flush = Instant::now();
                    Some(std::mem::take(&mut entry.buf))
                }
            })
        };

        if let Some(bytes) = bytes {
            (self.on_flush)(session_id, &bytes);
        }
    }

    /// Force-flush all pending bytes for every tracked session.
    pub fn flush_all(&self) {
        let pending: Vec<(String, Vec<u8>)> = {
            let mut map = self.inner.lock();
            map.iter_mut()
                .filter_map(|(sid, entry)| {
                    if entry.buf.is_empty() {
                        None
                    } else {
                        entry.last_flush = Instant::now();
                        Some((sid.clone(), std::mem::take(&mut entry.buf)))
                    }
                })
                .collect()
        };

        for (sid, bytes) in pending {
            (self.on_flush)(&sid, &bytes);
        }
    }

    /// Drop any buffered state for `session_id`. Call after explicit
    /// session removal so the map doesn't retain dead entries.
    pub fn forget(&self, session_id: &str) {
        self.inner.lock().remove(session_id);
    }
}
