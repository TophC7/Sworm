use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::session::Session;
use crate::services::codex_state::CodexStateReader;
use crate::services::nix::NixService;
use crate::services::providers::ProviderService;
use crate::services::pty::PtyEvent;
use crate::services::sessions::SessionService;
use crate::services::settings::SettingsService;
use chrono::{Duration, Utc};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use tracing::warn;

const CODEX_BIND_LOOKBACK_SECS: i64 = 15;
const CODEX_BIND_TIMEOUT_SECS: u64 = 20;
const CODEX_BIND_POLL_MS: u64 = 250;

fn spawn_codex_bind_thread(
    session_id: String,
    cwd: String,
    db_path: PathBuf,
    started_at: chrono::DateTime<Utc>,
    bind_lock: Arc<Mutex<()>>,
    all_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
) {
    thread::Builder::new()
        .name(format!("codex-bind-{}", &session_id))
        .spawn(move || {
            // Serialize binding per cwd; prevents two sessions from racing
            // to discover the same Codex thread. Released when this thread exits.
            let _guard = bind_lock.lock();

            let lookback = started_at - Duration::seconds(CODEX_BIND_LOOKBACK_SECS);
            let since = lookback.to_rfc3339();
            let deadline = Instant::now() + StdDuration::from_secs(CODEX_BIND_TIMEOUT_SECS);

            while Instant::now() <= deadline {
                match CodexStateReader::find_recent_threads_for_cwd(&cwd, &since) {
                    Ok(threads) if !threads.is_empty() => {
                        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                            let _ = conn
                                .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
                            let _ = SessionService::new().set_resume_token(
                                &conn,
                                &session_id,
                                &threads[0].id,
                            );
                            tracing::info!(
                                "Bound Codex thread {} to session {}",
                                threads[0].id,
                                session_id
                            );
                        }
                        return;
                    }
                    Ok(_) => {}
                    Err(error) => tracing::warn!(
                        "Failed polling Codex state for session {}: {}",
                        session_id,
                        error
                    ),
                }

                thread::sleep(StdDuration::from_millis(CODEX_BIND_POLL_MS));
            }

            match CodexStateReader::find_latest_thread_for_cwd(&cwd) {
                Ok(Some(thread_state)) => {
                    if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                        let _ =
                            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
                        let _ = SessionService::new().set_resume_token(
                            &conn,
                            &session_id,
                            &thread_state.id,
                        );
                        tracing::info!(
                            "Fallback bound Codex thread {} to session {}",
                            thread_state.id,
                            session_id
                        );
                    }
                }
                Ok(None) => {
                    tracing::warn!("No Codex thread discovered for session {}", session_id);
                }
                Err(error) => {
                    tracing::warn!(
                        "Fallback Codex bind failed for session {}: {}",
                        session_id,
                        error
                    );
                }
            }

            // Release the per-cwd lock, then evict the map entry if no other
            // bind thread is waiting on it. This prevents unbounded growth.
            drop(_guard);
            let mut locks = all_locks.lock();
            if let Some(arc) = locks.get(&cwd) {
                // strong_count == 1 means only the map holds it; safe to remove.
                if Arc::strong_count(arc) == 1 {
                    locks.remove(&cwd);
                }
            }
        })
        .map_err(|error| tracing::error!("Failed to spawn Codex bind thread: {}", error))
        .ok();
}

/// Create a new session for a project.
#[tauri::command]
pub async fn session_create(
    project_id: String,
    provider_id: String,
    title: String,
    state: tauri::State<'_, AppState>,
) -> Result<Session, ApiError> {
    if !ProviderService::exists(&provider_id) {
        return Err(ApiError::InvalidArgument(format!(
            "Unsupported provider: {}",
            provider_id
        )));
    }

    let db = state.db.write();
    if provider_id == "fresh" {
        // Fresh is a project-scoped singleton tool: multiple frontend
        // entry points may "open" it, but they should all resolve the
        // same persisted session row.
        if let Some(existing) = state
            .sessions
            .get_latest_for_project_provider(db.conn(), &project_id, &provider_id)
            .map_err(ApiError::Database)?
        {
            return Ok(existing);
        }
    }

    let project = state
        .projects
        .get(db.conn(), &project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

    let branch = state
        .git
        .current_branch(std::path::Path::new(&project.path));

    state
        .sessions
        .create(
            db.conn(),
            &project_id,
            &provider_id,
            &title,
            &project.path,
            branch.as_deref(),
        )
        .map_err(ApiError::Database)
}

/// List sessions for a project.
///
/// Reconciles stale statuses on the way out: any session that claims to be
/// running/starting but has no live PTY is marked as exited. This handles
/// app crashes, force-quits, and tabs closed without a clean status flush.
#[tauri::command]
pub async fn session_list(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, ApiError> {
    let db = state.db.write();
    let mut sessions = state
        .sessions
        .list_for_project(db.conn(), &project_id)
        .map_err(ApiError::Database)?;

    for s in &mut sessions {
        if (s.status == "running" || s.status == "starting") && !state.pty.is_alive(&s.id) {
            let _ = state.sessions.update_status(db.conn(), &s.id, "exited");
            s.status = "exited".to_string();
        }
    }

    Ok(sessions)
}

/// Get a single session.
#[tauri::command]
pub async fn session_get(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Session, ApiError> {
    // Pure read; route through the reader pool so a concurrent
    // write doesn't stall the UI's per-tab session fetch.
    let db = state.db.read();
    state
        .sessions
        .get(db.conn(), &id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", id)))
}

/// Start a session: spawn the provider CLI in a PTY.
#[tauri::command]
pub async fn session_start(
    session_id: String,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.write();
    let mut session = state
        .sessions
        .get(db.conn(), &session_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", session_id)))?;
    let provider_config = SettingsService::get_provider_config(db.conn(), &session.provider_id)
        .map_err(ApiError::Database)?
        .unwrap_or_else(|| {
            crate::services::settings::ProviderConfigRecord::default_for(&session.provider_id)
        });

    // Track whether this is the very first start (no token yet) so we can
    // choose --session-id (new) vs --resume (existing) when building args.
    let first_start = session.provider_resume_token.is_none();

    // On first start, generate and persist a resume token for providers that
    // support session resumption. Codex handles its own binding separately.
    if first_start {
        let token = match session.provider_id.as_str() {
            "claude_code" => Some(SessionService::deterministic_session_uuid(
                "claude",
                &session_id,
            )),
            "copilot" => Some(SessionService::deterministic_session_uuid(
                "copilot",
                &session_id,
            )),
            "codex" | "terminal" | "fresh" => None,
            // GenericFlag providers (Gemini, etc.): marker so restarts add resume flags
            _ => Some("started".to_string()),
        };
        if let Some(ref t) = token {
            state
                .sessions
                .set_resume_token(db.conn(), &session_id, t)
                .map_err(ApiError::Database)?;
        }
        // Only update in-memory token for providers with deterministic UUIDs.
        // GenericFlag providers keep provider_resume_token as None on first
        // start so the match block below produces (None, None) → fresh start.
        match session.provider_id.as_str() {
            "claude_code" | "copilot" => {
                session.provider_resume_token = token;
            }
            _ => {}
        }
    }

    if session.provider_id == "codex" {
        if let Some(token) = session.provider_resume_token.clone() {
            if !CodexStateReader::thread_exists(&token, &session.cwd).unwrap_or(false) {
                state
                    .sessions
                    .clear_resume_token(db.conn(), &session_id)
                    .map_err(ApiError::Database)?;
                session.provider_resume_token = None;
                warn!("Codex thread {} no longer exists, starting fresh", token);
            }
        }
    }

    state
        .sessions
        .update_status(db.conn(), &session_id, "starting")
        .map_err(ApiError::Database)?;

    let db_path = state.db.db_path().clone();

    // Load Nix env before dropping db; needed for both PATH resolution and child env
    let nix_env_vars =
        NixService::load_env_vars(db.conn(), &session.project_id).unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to load Nix env for project {}: {}",
                session.project_id,
                e
            );
            None
        });
    drop(db);

    // Use Nix-augmented PATH for command resolution when available
    let effective_path = match &nix_env_vars {
        Some(nix_env) => NixService::merged_path(&state.env.merged_path, nix_env),
        None => state.env.merged_path.clone(),
    };

    let cli_cmd = if session.provider_id == "terminal" {
        // Respect user override from settings, fall back to detected login shell
        provider_config
            .binary_path_override
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| state.env.detected_shell.clone())
    } else {
        ProviderService::resolve_command_path(
            &session.provider_id,
            &effective_path,
            provider_config.binary_path_override.as_deref(),
        )
        .unwrap_or_else(|| {
            ProviderService::cli_command(&session.provider_id)
                .unwrap_or("/bin/bash")
                .to_string()
        })
    };
    let (resume_token, session_app_id) = match session.provider_id.as_str() {
        "claude_code" => {
            // Claude CLI is NOT idempotent here:
            //   `claude --session-id <uuid>` creates a new session and
            //   errors with "Session ID <uuid> is already in use." if the
            //   transcript file already exists.
            //   `claude --resume <uuid>` resumes an existing session and
            //   errors with "No conversation found" if it doesn't.
            // So we must pick the right flag based on whether Claude has
            // already written a transcript for this UUID.
            let token = session.provider_resume_token.clone().unwrap_or_else(|| {
                SessionService::deterministic_session_uuid("claude", &session_id)
            });
            if crate::services::providers::claude_session_transcript_exists(&session.cwd, &token) {
                (Some(token), None)
            } else {
                (None, Some(token))
            }
        }
        "copilot" => {
            // Copilot uses --resume for both new and existing sessions
            let token = session.provider_resume_token.clone().unwrap_or_else(|| {
                SessionService::deterministic_session_uuid("copilot", &session_id)
            });
            (Some(token), None)
        }
        "codex" => (session.provider_resume_token.clone(), None),
        _ => {
            // GenericFlag providers (Gemini, etc.): resume_token signals restart
            if first_start {
                (None, None)
            } else {
                (session.provider_resume_token.clone(), None)
            }
        }
    };

    let mut args = ProviderService::build_start_args(
        &session.provider_id,
        session.auto_approve,
        resume_token.as_deref(),
        session_app_id.as_deref(),
        None,
    );
    args.extend(provider_config.extra_args);

    // Fresh: attach to a deterministic named session per project so multiple
    // tabs share one editor and we can reliably send files to it.
    if session.provider_id == "fresh" {
        let name = crate::commands::fresh::fresh_session_name(&session.project_id);
        args.insert(0, name);
        args.insert(0, "-a".to_string());
    }

    let arg_refs: Vec<&str> = args.iter().map(|value| value.as_str()).collect();

    // Build child env: merge Nix environment if available
    let child_env = match nix_env_vars {
        Some(nix_env) => NixService::merge_env(&state.env.child_env, &nix_env),
        None => state.env.child_env.clone(),
    };

    // Flush the transcript batcher first so the final bytes before exit
    // reach disk; otherwise the last ~500 ms of output would be dropped
    // when the child dies.
    let batcher_for_exit = Arc::clone(&state.transcript_batcher);
    let on_exit: Box<dyn FnOnce(&str, Option<i32>) + Send> = Box::new(
        move |sid: &str, exit_code: Option<i32>| {
            batcher_for_exit.flush(sid);
            match rusqlite::Connection::open(&db_path) {
                Ok(conn) => {
                    let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
                    let now = chrono::Utc::now().to_rfc3339();
                    let _ = conn.execute(
                        "UPDATE sessions SET status = 'exited', updated_at = ?1, last_stopped_at = ?2 WHERE id = ?3",
                        rusqlite::params![now, now, sid],
                    );
                    tracing::info!(
                        "Backend marked session {} as exited with code {:?}",
                        sid,
                        exit_code
                    );
                }
                Err(error) => {
                    tracing::error!("Failed to open DB for exit callback: {}", error);
                }
            }
        },
    );

    let started_at = Utc::now();
    let spawn_result = state.pty.spawn(
        session_id.clone(),
        &cli_cmd,
        &arg_refs,
        Some(&session.cwd),
        Some(&child_env),
        cols,
        rows,
        output,
        events,
        true,
        Some(on_exit),
    );

    let db = state.db.write();
    match &spawn_result {
        Ok(()) => {
            state
                .sessions
                .update_status(db.conn(), &session_id, "running")
                .map_err(ApiError::Database)?;

            if session.provider_id == "codex" && session.provider_resume_token.is_none() {
                let bind_lock = {
                    let mut locks = state.codex_bind_locks.lock();
                    locks
                        .entry(session.cwd.clone())
                        .or_insert_with(|| Arc::new(Mutex::new(())))
                        .clone()
                };
                spawn_codex_bind_thread(
                    session_id.clone(),
                    session.cwd.clone(),
                    state.db.db_path().clone(),
                    started_at,
                    bind_lock,
                    Arc::clone(&state.codex_bind_locks),
                );
            }
        }
        Err(_) => {
            state
                .sessions
                .update_status(db.conn(), &session_id, "failed")
                .map_err(ApiError::Database)?;
        }
    }

    spawn_result.map_err(ApiError::Pty)
}

/// Write input to a running session's PTY.
#[tauri::command]
pub async fn session_write(
    session_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.write(&session_id, &data).map_err(ApiError::Pty)
}

/// Resize a running session's PTY.
#[tauri::command]
pub async fn session_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .pty
        .resize(&session_id, cols, rows)
        .map_err(ApiError::Pty)
}

/// Stop a running session.
#[tauri::command]
pub async fn session_stop(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);
    // Persist whatever transcript bytes were still in-flight. Safe to
    // call even when the session was never live; the batcher no-ops.
    state.transcript_batcher.flush(&session_id);

    let db = state.db.write();
    state
        .sessions
        .update_status(db.conn(), &session_id, "stopped")
        .map_err(ApiError::Database)?;

    Ok(())
}

/// Reset a session to fresh state (new deterministic ID, cleared history).
/// Use when a session is stuck or its provider-side conversation is gone.
#[tauri::command]
pub async fn session_reset(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);

    let db = state.db.write();
    let session = state
        .sessions
        .get(db.conn(), &session_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", session_id)))?;

    // Generate a fresh deterministic token for providers that use session IDs
    let new_token = match session.provider_id.as_str() {
        "claude_code" => Some(SessionService::deterministic_session_uuid(
            "claude",
            &session_id,
        )),
        "copilot" => Some(SessionService::deterministic_session_uuid(
            "copilot",
            &session_id,
        )),
        _ => None,
    };

    state
        .sessions
        .reset_session(db.conn(), &session_id, new_token.as_deref())
        .map_err(ApiError::Database)?;

    Ok(())
}

/// Delete a session.
///
/// Cleans up every related resource: the live PTY (if any), pending
/// transcript batch, and all durable transcript rows. The row-level
/// delete also happens via `ON DELETE CASCADE` on the session row,
/// but clearing here first avoids ferrying dead entries through the
/// foreign-key path.
#[tauri::command]
pub async fn session_remove(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);
    state.transcript_batcher.forget(&session_id);
    state.transcript.clear(&session_id);

    let db = state.db.write();
    state
        .sessions
        .remove(db.conn(), &session_id)
        .map_err(ApiError::Database)
}

/// Archive a session. Stops its PTY if running, then marks it archived.
#[tauri::command]
pub async fn session_archive(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    // Stop the PTY if it's alive; archived sessions are never running
    let _ = state.pty.kill(&session_id);

    let db = state.db.write();

    // Ensure the session exists before archiving
    let session = state
        .sessions
        .get(db.conn(), &session_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", session_id)))?;

    // Mark as stopped if it was running/starting
    if session.status == "running" || session.status == "starting" {
        state
            .sessions
            .update_status(db.conn(), &session_id, "stopped")
            .map_err(ApiError::Database)?;
    }

    state
        .sessions
        .archive(db.conn(), &session_id)
        .map_err(ApiError::Database)
}

/// Unarchive a session. Restores it to the active session list.
#[tauri::command]
pub async fn session_unarchive(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.write();

    // Ensure the session exists before unarchiving
    let _session = state
        .sessions
        .get(db.conn(), &session_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", session_id)))?;

    state
        .sessions
        .unarchive(db.conn(), &session_id)
        .map_err(ApiError::Database)
}

/// List archived sessions for a project.
#[tauri::command]
pub async fn session_list_archived(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, ApiError> {
    // Pure read; uses the reader pool.
    let db = state.db.read();
    state
        .sessions
        .list_archived_for_project(db.conn(), &project_id)
        .map_err(ApiError::Database)
}
