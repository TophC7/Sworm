use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::session::Session;
use crate::services::codex_state::CodexStateReader;
use crate::services::nix::NixService;
use crate::services::providers::ProviderService;
use crate::services::pty::PtyEvent;
use crate::services::settings::SettingsService;
use crate::services::sessions::SessionService;
use chrono::{Duration, Utc};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration as StdDuration, Instant};
use tracing::warn;

const CODEX_BIND_LOOKBACK_SECS: i64 = 15;
const CODEX_BIND_TIMEOUT_SECS: u64 = 20;
const CODEX_BIND_POLL_MS: u64 = 250;

fn spawn_codex_bind_thread(session_id: String, cwd: String, db_path: PathBuf, started_at: chrono::DateTime<Utc>) {
    thread::Builder::new()
        .name(format!("codex-bind-{}", &session_id))
        .spawn(move || {
            let lookback = started_at - Duration::seconds(CODEX_BIND_LOOKBACK_SECS);
            let since = lookback.to_rfc3339();
            let deadline = Instant::now() + StdDuration::from_secs(CODEX_BIND_TIMEOUT_SECS);

            while Instant::now() <= deadline {
                match CodexStateReader::find_recent_threads_for_cwd(&cwd, &since) {
                    Ok(threads) if !threads.is_empty() => {
                        if let Ok(conn) = rusqlite::Connection::open(&db_path) {
                            let _ = conn.execute_batch(
                                "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;",
                            );
                            let _ = SessionService::new()
                                .set_resume_token(&conn, &session_id, &threads[0].id);
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
                        let _ = conn.execute_batch(
                            "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;",
                        );
                        let _ = SessionService::new()
                            .set_resume_token(&conn, &session_id, &thread_state.id);
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
        })
        .map_err(|error| tracing::error!("Failed to spawn Codex bind thread: {}", error))
        .ok();
}

/// Create a new session for a project.
#[tauri::command]
pub fn session_create(
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

    let db = state.db.lock();
    let project = state
        .projects
        .get(db.conn(), &project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;

    let branch = state.git.current_branch(std::path::Path::new(&project.path));

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
#[tauri::command]
pub fn session_list(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Session>, ApiError> {
    let db = state.db.lock();
    state
        .sessions
        .list_for_project(db.conn(), &project_id)
        .map_err(ApiError::Database)
}

/// Get a single session.
#[tauri::command]
pub fn session_get(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Session, ApiError> {
    let db = state.db.lock();
    state
        .sessions
        .get(db.conn(), &id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", id)))
}

/// Start a session: spawn the provider CLI in a PTY.
#[tauri::command]
pub fn session_start(
    session_id: String,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let db = state.db.lock();
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

    if session.provider_id == "codex" && session.provider_resume_token.is_none() {
        let live_unbound = state
            .sessions
            .count_unbound_codex_for_project(db.conn(), &session.project_id)
            .map_err(ApiError::Database)?;
        if live_unbound > 0 {
            return Err(ApiError::InvalidArgument(
                "Another Codex session is still binding to a thread. Wait for it to complete or stop it first.".to_string(),
            ));
        }
    }

    if session.provider_id == "claude_code" && session.provider_resume_token.is_none() {
        let token = SessionService::deterministic_claude_session_id(&session_id);
        state
            .sessions
            .set_resume_token(db.conn(), &session_id, &token)
            .map_err(ApiError::Database)?;
        session.provider_resume_token = Some(token);
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

    let db_path = db.db_path().clone();

    // Load Nix env before dropping db — needed for both PATH resolution and child env
    let nix_env_vars = NixService::load_env_vars(db.conn(), &session.project_id)
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to load Nix env for project {}: {}", session.project_id, e);
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
            // Claude Code uses --session-id <uuid> for both new and existing
            // sessions. Always pass as session_app_id (never resume_token)
            // so build_start_args emits `--session-id <uuid>` without --resume.
            let token = session
                .provider_resume_token
                .clone()
                .unwrap_or_else(|| SessionService::deterministic_claude_session_id(&session_id));
            (None, Some(token))
        }
        "codex" => (session.provider_resume_token.clone(), None),
        _ => (None, None),
    };

    let mut args = ProviderService::build_start_args(
        &session.provider_id,
        session.auto_approve,
        resume_token.as_deref(),
        session_app_id.as_deref(),
        None,
    );
    args.extend(provider_config.extra_args);
    let arg_refs: Vec<&str> = args.iter().map(|value| value.as_str()).collect();

    // Build child env: merge Nix environment if available
    let child_env = match nix_env_vars {
        Some(nix_env) => NixService::merge_env(&state.env.child_env, &nix_env),
        None => state.env.child_env.clone(),
    };

    let on_exit: Box<dyn FnOnce(&str, Option<i32>) + Send> =
        Box::new(move |sid: &str, exit_code: Option<i32>| match rusqlite::Connection::open(&db_path)
        {
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
        });

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
        Some(on_exit),
    );

    let db = state.db.lock();
    match &spawn_result {
        Ok(()) => {
            state
                .sessions
                .update_status(db.conn(), &session_id, "running")
                .map_err(ApiError::Database)?;

            if session.provider_id == "codex" && session.provider_resume_token.is_none() {
                spawn_codex_bind_thread(
                    session_id.clone(),
                    session.cwd.clone(),
                    db.db_path().clone(),
                    started_at,
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
pub fn session_write(
    session_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.write(&session_id, &data).map_err(ApiError::Pty)
}

/// Resize a running session's PTY.
#[tauri::command]
pub fn session_resize(
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
pub fn session_stop(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);

    let db = state.db.lock();
    state
        .sessions
        .update_status(db.conn(), &session_id, "stopped")
        .map_err(ApiError::Database)?;

    Ok(())
}

/// Reset a session to fresh state (new deterministic ID, cleared history).
/// Use when a session is stuck or its provider-side conversation is gone.
#[tauri::command]
pub fn session_reset(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);

    let db = state.db.lock();
    let session = state
        .sessions
        .get(db.conn(), &session_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Session not found: {}", session_id)))?;

    // Generate a fresh deterministic token for Claude Code
    let new_token = if session.provider_id == "claude_code" {
        Some(SessionService::deterministic_claude_session_id(&session_id))
    } else {
        None
    };

    state
        .sessions
        .reset_session(db.conn(), &session_id, new_token.as_deref())
        .map_err(ApiError::Database)?;

    Ok(())
}

/// Delete a session.
#[tauri::command]
pub fn session_remove(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);

    let db = state.db.lock();
    state
        .sessions
        .remove(db.conn(), &session_id)
        .map_err(ApiError::Database)
}
