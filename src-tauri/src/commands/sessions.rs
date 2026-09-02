use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::session::SessionStartInfo;
use crate::services::codex_state::CodexStateReader;
use crate::services::folders::resolve_folder;
use crate::services::nix::NixService;
use crate::services::omp;
use crate::services::providers::{
    antigravity_conversation_exists, antigravity_find_new_conversation,
    claude_session_transcript_exists, deterministic_session_uuid, ProviderService,
};
use crate::services::pty::PtyEvent;
use crate::services::settings_resolution::{
    provider_config_record, resolve_effective_settings_for_project_path,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tracing::{info, warn};

/// How far before spawn a provider-side conversation may have been
/// created and still be attributed to this session.
const BIND_LOOKBACK: Duration = Duration::from_secs(15);
const BIND_TIMEOUT: Duration = Duration::from_secs(20);
const BIND_POLL: Duration = Duration::from_millis(250);

type BindLocks = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;

/// Discover the provider-side conversation a freshly spawned session
/// created, then announce it over `events` as `ResumeTokenBound`.
///
/// `poll` runs every `BIND_POLL` until `BIND_TIMEOUT`. Only provider
/// state created or modified after spawn is eligible; binding an older
/// conversation would silently resume the wrong user's context.
/// On timeout the session stays unbound and restarts fresh.
fn spawn_bind_thread(
    label: &'static str,
    session_id: String,
    cwd: String,
    events: tauri::ipc::Channel<PtyEvent>,
    all_locks: BindLocks,
    mut poll: impl FnMut() -> Option<String> + Send + 'static,
) {
    let bind_lock = all_locks
        .lock()
        .entry(cwd.clone())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone();

    thread::Builder::new()
        .name(format!("{label}-bind-{session_id}"))
        .spawn(move || {
            let guard = bind_lock.lock();

            let deadline = Instant::now() + BIND_TIMEOUT;
            let mut token = None;
            while token.is_none() && Instant::now() <= deadline {
                token = poll();
                if token.is_none() {
                    thread::sleep(BIND_POLL);
                }
            }

            match token {
                Some(token) => {
                    info!("Bound {label} conversation {token} to session {session_id}");
                    let _ = events.send(PtyEvent::ResumeTokenBound { session_id, token });
                }
                None => warn!("No {label} conversation discovered for session {session_id}"),
            }

            // Release the per-cwd lock, then evict the map entry if no other
            // bind thread holds it. Prevents unbounded growth.
            drop(guard);
            drop(bind_lock);
            let mut locks = all_locks.lock();
            if locks
                .get(&cwd)
                .is_some_and(|arc| Arc::strong_count(arc) == 1)
            {
                locks.remove(&cwd);
            }
        })
        .map_err(|error| tracing::error!("Failed to spawn {label} bind thread: {error}"))
        .ok();
}

fn spawn_codex_bind_thread(
    session_id: String,
    cwd: String,
    events: tauri::ipc::Channel<PtyEvent>,
    all_locks: BindLocks,
) {
    let since = (chrono::Utc::now()
        - chrono::Duration::from_std(BIND_LOOKBACK).unwrap_or_default())
    .to_rfc3339();
    let poll_cwd = cwd.clone();
    spawn_bind_thread("codex", session_id, cwd, events, all_locks, move || {
        match CodexStateReader::find_recent_threads_for_cwd(&poll_cwd, &since) {
            Ok(threads) => threads.into_iter().next().map(|thread| thread.id),
            Err(error) => {
                warn!("Failed polling Codex state: {error}");
                None
            }
        }
    });
}

fn spawn_antigravity_bind_thread(
    session_id: String,
    cwd: String,
    events: tauri::ipc::Channel<PtyEvent>,
    all_locks: BindLocks,
    bound_ids: Arc<Mutex<HashSet<String>>>,
) {
    let since = SystemTime::now() - BIND_LOOKBACK;
    let poll_ids = Arc::clone(&bound_ids);
    spawn_bind_thread(
        "antigravity",
        session_id,
        cwd,
        events,
        all_locks,
        move || {
            // Scope the read guard: re-locking below would deadlock.
            let id = {
                let ids = poll_ids.lock();
                antigravity_find_new_conversation(since, &ids)
            }?;
            poll_ids.lock().insert(id.clone()).then_some(id)
        },
    );
}

/// Start a session: spawn the provider CLI in a PTY inside `folder_path`.
///
/// Resume semantics per provider:
/// - Claude Code: token is always derived from `session_id` (announced via
///   `ResumeTokenBound`); `--resume` when its transcript exists on disk,
///   else `--session-id`.
/// - Codex: `resume <thread>` when the supplied thread exists in Codex's
///   state DB for this cwd; otherwise fresh and a bind thread discovers
///   the new thread id.
/// - OMP: private `--session-dir`; `--continue` when that dir already
///   holds state.
/// - Antigravity: `--conversation <id>` when the supplied conversation
///   store exists; otherwise fresh and a bind thread discovers the id.
/// - Terminal: never resumes.
#[tauri::command]
pub async fn session_start(
    session_id: String,
    folder_path: String,
    provider_id: String,
    resume_token: Option<String>,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<SessionStartInfo, ApiError> {
    if !ProviderService::exists(&provider_id) {
        return Err(ApiError::InvalidArgument(format!(
            "Unsupported provider: {provider_id}"
        )));
    }
    let folder = resolve_folder(&folder_path)?;
    let cwd = folder.to_string_lossy().into_owned();

    let effective_settings = resolve_effective_settings_for_project_path(Some(folder.as_path()))
        .map_err(ApiError::Internal)?;
    let provider_config = provider_config_record(&effective_settings.settings, &provider_id);
    if !provider_config.enabled {
        return Err(ApiError::InvalidArgument(format!(
            "Provider disabled by settings: {provider_id}"
        )));
    }

    let nix_env_vars = {
        let db = state.db.read();
        NixService::load_env_vars(db.conn(), &cwd).unwrap_or_else(|error| {
            warn!("Failed to load Nix env for folder {cwd}: {error}");
            None
        })
    };

    // Use Nix-augmented PATH for command resolution when available
    let effective_path = match &nix_env_vars {
        Some(nix_env) => NixService::merged_path(&state.env.merged_path, nix_env),
        None => state.env.merged_path.clone(),
    };

    let cli_cmd = if provider_id == "terminal" {
        // Respect user override from settings, fall back to detected login shell
        provider_config
            .binary_path_override
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| state.env.detected_shell.clone())
    } else {
        ProviderService::resolve_command_path(
            &provider_id,
            &effective_path,
            provider_config.binary_path_override.as_deref(),
        )
        .unwrap_or_else(|| {
            ProviderService::cli_command(&provider_id)
                .unwrap_or("/bin/bash")
                .to_string()
        })
    };

    // Provider-side identity: Claude derives it, Codex/Antigravity validate
    // the supplied token on disk. An invalid token means a fresh start.
    let (resume_token, session_app_id) = match provider_id.as_str() {
        "claude_code" => {
            // Claude CLI is NOT idempotent here:
            //   `claude --session-id <uuid>` errors if the transcript exists,
            //   `claude --resume <uuid>` errors if it doesn't.
            let token = deterministic_session_uuid("claude", &session_id);
            if claude_session_transcript_exists(&cwd, &token) {
                (Some(token), None)
            } else {
                (None, Some(token))
            }
        }
        "codex" => (
            resume_token.filter(|token| {
                let exists = CodexStateReader::thread_exists(token, &cwd).unwrap_or(false);
                if !exists {
                    warn!("Codex thread {token} no longer exists, starting fresh");
                }
                exists
            }),
            None,
        ),
        "antigravity" => (
            resume_token.filter(|token| {
                let exists = antigravity_conversation_exists(token);
                if !exists {
                    warn!("Antigravity conversation {token} no longer exists, starting fresh");
                }
                exists
            }),
            None,
        ),
        _ => (None, None),
    };
    let mut resumed = resume_token.is_some();

    let mut args = ProviderService::build_start_args(
        &provider_id,
        resume_token.as_deref(),
        session_app_id.as_deref(),
        None,
    );
    args.extend(provider_config.extra_args);

    // Isolate this Sworm session from OMP's global store. The private
    // directory lives until the tab is discarded.
    if provider_id == "omp" {
        let omp_session_dir = omp::ensure_session_dir(state.db.app_data_dir(), &session_id)
            .map_err(|error| ApiError::Io(error.to_string()))?;
        let has_state = omp::session_dir_has_state(&omp_session_dir);
        args.push("--session-dir".to_string());
        args.push(omp_session_dir.to_string_lossy().into_owned());
        if has_state {
            args.push("--continue".to_string());
            resumed = true;
        }
    }

    let arg_refs: Vec<&str> = args.iter().map(|value| value.as_str()).collect();

    // Build child env: merge Nix environment if available
    let mut child_env = match nix_env_vars {
        Some(nix_env) => NixService::merge_env(&state.env.child_env, &nix_env),
        None => state.env.child_env.clone(),
    };

    // Agent sessions get Sworm issue-memory bridge coordinates after
    // environment merge so Sworm runtime values win over inherited env.
    if provider_id != "terminal" {
        match state.issue_bridge.ensure_running(&folder) {
            Ok(info) => {
                child_env.insert("SWORM_PROJECT_PATH".to_string(), info.project_path);
                child_env.insert("SWORM_ISSUES_SOCKET".to_string(), info.socket_path);
                child_env.insert("SWORM_ISSUES_TOKEN".to_string(), info.token);
                child_env.insert(
                    "SWORM_ISSUES_PROTOCOL_VERSION".to_string(),
                    info.protocol_version.to_string(),
                );
            }
            Err(error) => {
                warn!("Issue bridge unavailable for session {session_id} in {cwd}: {error}");
            }
        }
    }

    let on_exit: Box<dyn FnOnce(&str, Option<i32>) + Send> =
        Box::new(|sid, code| info!("Session {sid} exited with code {code:?}"));

    state
        .pty
        .spawn(
            session_id.clone(),
            &cli_cmd,
            &arg_refs,
            Some(&cwd),
            Some(&child_env),
            cols,
            rows,
            output,
            events.clone(),
            Some(on_exit),
        )
        .map_err(ApiError::Pty)?;

    match (provider_id.as_str(), resume_token) {
        // Claude's identity is deterministic; announcing it lets a restored
        // tab supply a token, so a vanished transcript surfaces as
        // `resumed: false` against a non-null token (same UX as Codex).
        // Exactly one of the two carries the id, depending on transcript presence.
        ("claude_code", resume) => {
            if let Some(token) = resume.or(session_app_id) {
                let _ = events.send(PtyEvent::ResumeTokenBound { session_id, token });
            }
        }
        ("codex", None) => {
            spawn_codex_bind_thread(session_id, cwd, events, Arc::clone(&state.bind_locks));
        }
        ("antigravity", None) => spawn_antigravity_bind_thread(
            session_id,
            cwd,
            events,
            Arc::clone(&state.bind_locks),
            Arc::clone(&state.bound_conversation_ids),
        ),
        // Claim the resumed conversation so a sibling tab's bind thread
        // can't attach to it.
        ("antigravity", Some(token)) => {
            state.bound_conversation_ids.lock().insert(token);
        }
        _ => {}
    }

    Ok(SessionStartInfo { resumed })
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

/// Stop a running session. No-op when no PTY is live.
#[tauri::command]
pub async fn session_stop(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);
    Ok(())
}

/// Whether a live PTY currently exists for `session_id` in this process.
#[tauri::command]
pub async fn session_is_alive(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<bool, ApiError> {
    Ok(state.pty.is_alive(&session_id))
}

/// Forget a session for good: kill its PTY and drop its OMP state dir.
/// Called when a closed tab falls off the reopen stack.
#[tauri::command]
pub async fn session_discard(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let _ = state.pty.kill(&session_id);
    omp::remove_session_dir(state.db.app_data_dir(), &session_id);
    Ok(())
}

/// Drop OMP state dirs for sessions no restored tab references.
#[tauri::command]
pub async fn session_prune_orphans(
    keep_session_ids: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let keep: HashSet<String> = keep_session_ids.into_iter().collect();
    omp::remove_session_dirs_except(state.db.app_data_dir(), &keep);
    Ok(())
}
