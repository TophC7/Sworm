use parking_lot::Mutex;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

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
    runtime_id: String,
}

/// PTY service managing all active pseudo-terminal sessions.
pub struct PtyService {
    sessions: Arc<Mutex<HashMap<String, LivePty>>>,
    finalized_ptys: Arc<Mutex<HashSet<String>>>,
}

impl PtyService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            finalized_ptys: Arc::new(Mutex::new(HashSet::new())),
        }
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
        let runtime_id = Uuid::new_v4().to_string();

        self.finalized_ptys.lock().remove(&runtime_id);
        self.sessions.lock().insert(
            session_id.clone(),
            LivePty {
                master: pair.master,
                writer,
                killer,
                shutdown: shutdown.clone(),
                runtime_id: runtime_id.clone(),
            },
        );

        let _ = event_channel.send(PtyEvent::Started {
            session_id: session_id.clone(),
            pid,
        });

        let sid_for_thread = session_id.clone();
        let sessions_for_thread = Arc::clone(&self.sessions);
        let finalized_for_thread = Arc::clone(&self.finalized_ptys);
        std::thread::Builder::new()
            .name(format!("pty-reader-{}", &session_id))
            .spawn(move || {
                let mut buf = [0u8; 4096];

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
                            if output_channel.send(buf[..n].to_vec()).is_err() {
                                detached.store(true, Ordering::Relaxed);
                                info!("Output channel closed for {}, detaching frontend", sid_for_thread);
                                break;
                            }
                        }
                        Err(err) => {
                            if shutdown.load(Ordering::Relaxed) {
                                info!("PTY read loop stopped during shutdown for {}", sid_for_thread);
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

                let finalized_now = {
                    let mut finalized = finalized_for_thread.lock();
                    finalized.insert(runtime_id.clone())
                };

                if !finalized_now {
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
            self.finalized_ptys.lock().insert(live.runtime_id.clone());

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
            self.finalized_ptys.lock().insert(live.runtime_id.clone());

            if let Err(err) = live.killer.kill() {
                warn!("Failed to kill PTY child {} during shutdown: {}", session_id, err);
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
