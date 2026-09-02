use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

// PTY reader buffer. PTYs deliver up to whatever the buffer holds in a
// single read; a 4 KiB buffer fragments bursts into many small reads,
// each of which used to fire its own IPC message. 64 KiB lets a typical
// agent burst land in one or two reads.
const PTY_READ_BUF_SIZE: usize = 64 * 1024;
// Output coalescing tick. Tauri's `Channel<Vec<u8>>` serializes via
// `serde_json` (Vec<u8> -> JSON array of numbers), which is parsed on
// the webview's main thread. The flusher batches bytes per-session and
// emits at most one IPC message per tick; 16 ms keeps perceived
// latency below one frame at 60 Hz while collapsing typical bursts
// into a single send. Going larger (32 ms) was tried and didn't move
// the freeze needle — the dominant cost is downstream, in xterm's
// per-session writeBuffer parsing across N concurrent sessions, not
// in the IPC tick rate.
const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);

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
}

impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn a new PTY running the given command.
    ///
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
        on_exit: Option<Box<dyn FnOnce(&str, Option<i32>) + Send>>,
    ) -> Result<(), String> {
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
        let detached = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));
        let runtime_id = uuid::Uuid::new_v4().to_string();

        self.sessions.lock().insert(
            run_id.clone(),
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
            run_id: run_id.clone(),
            pid,
        });

        let sid_for_thread = run_id.clone();
        let sessions_for_thread = Arc::clone(&self.sessions);

        std::thread::Builder::new()
            .name(format!("pty-reader-{}", &run_id))
            .spawn(move || {
                let mut buf = [0u8; PTY_READ_BUF_SIZE];

                // Output coalescing: reader appends bytes to `pending`;
                // the flusher thread drains and emits one IPC message
                // per tick. Single sender preserves byte order to xterm.
                let pending: Arc<Mutex<Vec<u8>>> =
                    Arc::new(Mutex::new(Vec::with_capacity(64 * 1024)));
                let flusher_stop = Arc::new(AtomicBool::new(false));

                let pending_for_flusher = Arc::clone(&pending);
                let flusher_stop_inner = Arc::clone(&flusher_stop);
                let detached_for_flusher = Arc::clone(&detached);
                let output_channel_for_flusher = output_channel.clone();
                let sid_for_flusher = sid_for_thread.clone();

                let flusher_handle = std::thread::Builder::new()
                    .name(format!("pty-flusher-{}", &sid_for_thread))
                    .spawn(move || {
                        // Periodic stats so we can verify batching is
                        // working when running an agent. Logged at info
                        // level (visible by default) once per second
                        // when there has been activity.
                        let mut window_flushes: u64 = 0;
                        let mut window_bytes: u64 = 0;
                        let mut window_max_batch: usize = 0;
                        let mut last_report = Instant::now();

                        loop {
                            std::thread::sleep(OUTPUT_FLUSH_INTERVAL);

                            let bytes_opt: Option<Vec<u8>> = {
                                let mut p = pending_for_flusher.lock();
                                if p.is_empty() {
                                    None
                                } else {
                                    Some(std::mem::take(&mut *p))
                                }
                            };

                            if let Some(bytes) = bytes_opt {
                                let n = bytes.len();
                                if !detached_for_flusher.load(Ordering::Relaxed) {
                                    if output_channel_for_flusher.send(bytes).is_err() {
                                        detached_for_flusher.store(true, Ordering::Relaxed);
                                        info!(
                                            "Output channel closed for {}, continuing to drain",
                                            sid_for_flusher
                                        );
                                    } else {
                                        window_flushes += 1;
                                        window_bytes += n as u64;
                                        if n > window_max_batch {
                                            window_max_batch = n;
                                        }
                                    }
                                }
                            }

                            if last_report.elapsed() >= Duration::from_secs(1) && window_flushes > 0
                            {
                                let secs = last_report.elapsed().as_secs_f64();
                                info!(
                                    "[pty-batcher {}] {:.0} flushes/s, {:.0} KiB/s, max batch {} B",
                                    sid_for_flusher,
                                    window_flushes as f64 / secs,
                                    window_bytes as f64 / secs / 1024.0,
                                    window_max_batch
                                );
                                window_flushes = 0;
                                window_bytes = 0;
                                window_max_batch = 0;
                                last_report = Instant::now();
                            }

                            if flusher_stop_inner.load(Ordering::Relaxed) {
                                break;
                            }
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
                        Ok(n) => {
                            // Append to the coalescing buffer. The
                            // flusher thread is the single owner of the
                            // IPC send path — keeping all sends serial
                            // preserves byte order for xterm.
                            if !detached.load(Ordering::Relaxed) {
                                pending.lock().extend_from_slice(&buf[..n]);
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
                                    run_id: sid_for_thread.clone(),
                                    message: err.to_string(),
                                });
                            }
                            break;
                        }
                    }
                }

                // Drain remaining bytes via the flusher BEFORE the Exit
                // event; otherwise the JS side sees the exit notification
                // before the tail of the program's last output.
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
                    run_id: sid_for_thread,
                    code: exit_code,
                });
            })
            .map_err(|e| format!("Failed to spawn reader thread: {}", e))?;

        Ok(())
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
            .map_err(|e| format!("PTY flush failed: {}", e))?;
        Ok(())
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
            .map_err(|e| format!("PTY resize failed: {}", e))?;

        Ok(())
    }

    /// Kill the PTY process and clean up.
    pub fn kill(&self, run_id: &str) -> Result<(), String> {
        let live = self.sessions.lock().remove(run_id);
        if let Some(mut live) = live {
            live.shutdown.store(true, Ordering::Relaxed);
            // Pre-mark finalized so the reader thread won't double-fire
            // the exit callback after we've initiated shutdown.
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
