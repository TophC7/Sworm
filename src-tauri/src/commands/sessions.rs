use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::provider::ProviderId;
use crate::models::session::SessionStartInfo;
use crate::services::codex_state::CodexStateReader;
use crate::services::folders::resolve_folder;
use crate::services::nix::NixService;
use crate::services::omp;
use crate::services::providers::{
    antigravity_conversation_exists, claude_session_transcript_exists, ProviderService,
};
use crate::services::pty::PtyEvent;
use crate::services::resume_discovery::PendingRun;
use crate::services::settings_resolution::{
    provider_config_record, resolve_effective_settings_for_folder_path,
};
use std::time::SystemTime;
use tracing::{info, warn};

/// Keep `token` only when `exists`; otherwise log that the provider's
/// conversation vanished and start fresh.
fn validated_token(
    token: Option<String>,
    label: &str,
    exists: impl FnOnce(&str) -> bool,
) -> Option<String> {
    token.filter(|token| {
        let exists = exists(token);
        if !exists {
            warn!("{label} {token} no longer exists, starting fresh");
        }
        exists
    })
}

/// Start a run: spawn the provider CLI in a PTY inside `folder_path`.
///
/// `run_id` identifies this PTY only; the durable identity is the tab,
/// which the frontend keeps. Resume semantics per provider:
/// - Claude Code: `--resume <token>` when the supplied token's transcript
///   exists on disk, else `--session-id <fresh uuid>`; the token in use
///   is returned immediately.
/// - Codex: `resume <thread>` when the supplied thread exists in Codex's
///   state DB for this cwd; otherwise fresh and discovery announces the
///   new thread id.
/// - OMP: `--resume <id>` when the supplied session file exists in the
///   folder's bucket; otherwise fresh and discovery announces the id.
/// - Antigravity: `--conversation <id>` when the supplied conversation
///   store exists; otherwise fresh and discovery announces the id.
/// - Terminal: never resumes.
#[tauri::command]
pub async fn session_start(
    run_id: String,
    folder_path: String,
    provider_id: String,
    resume_token: Option<String>,
    cols: u16,
    rows: u16,
    output: tauri::ipc::Channel<Vec<u8>>,
    events: tauri::ipc::Channel<PtyEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<SessionStartInfo, ApiError> {
    let provider = ProviderService::definition(&provider_id)
        .map(|definition| definition.id)
        .ok_or_else(|| ApiError::InvalidArgument(format!("Unsupported provider: {provider_id}")))?;
    let folder = resolve_folder(&folder_path)?;
    let cwd = folder.to_string_lossy().into_owned();

    let effective_settings = resolve_effective_settings_for_folder_path(Some(folder.as_path()))
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

    let cli_cmd = if provider == ProviderId::Terminal {
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

    // Provider-side identity: every supplied token is validated on disk;
    // an invalid token means a fresh start.
    let (resume_token, session_app_id) = match provider {
        // Claude CLI is NOT idempotent here:
        //   `claude --session-id <uuid>` errors if the transcript exists,
        //   `claude --resume <uuid>` errors if it doesn't.
        ProviderId::ClaudeCode => match validated_token(resume_token, "Claude session", |token| {
            claude_session_transcript_exists(&cwd, token)
        }) {
            Some(token) => (Some(token), None),
            None => (None, Some(uuid::Uuid::new_v4().to_string())),
        },
        ProviderId::Codex => (
            validated_token(resume_token, "Codex thread", |token| {
                CodexStateReader::thread_exists(token, &cwd).unwrap_or(false)
            }),
            None,
        ),
        ProviderId::Omp => (
            validated_token(resume_token, "OMP session", |token| {
                omp::session_exists(&cwd, token)
            }),
            None,
        ),
        ProviderId::Antigravity => (
            validated_token(resume_token, "Antigravity conversation", |token| {
                antigravity_conversation_exists(token)
            }),
            None,
        ),
        ProviderId::Terminal => (None, None),
    };
    let resumed = resume_token.is_some();

    let mut args = ProviderService::build_start_args(
        &provider_id,
        resume_token.as_deref(),
        session_app_id.as_deref(),
    );
    args.extend(provider_config.extra_args);
    let arg_refs: Vec<&str> = args.iter().map(|value| value.as_str()).collect();

    // Build child env: merge Nix environment if available
    let mut child_env = match nix_env_vars {
        Some(nix_env) => NixService::merge_env(&state.env.child_env, &nix_env),
        None => state.env.child_env.clone(),
    };

    // Agent sessions get Sworm issue-memory bridge coordinates after
    // environment merge so Sworm runtime values win over inherited env.
    if provider != ProviderId::Terminal {
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
                warn!("Issue bridge unavailable for run {run_id} in {cwd}: {error}");
            }
        }
    }

    let discovery = state.resume_discovery.clone();
    let on_exit: Box<dyn FnOnce(&str, Option<i32>) + Send> = Box::new(move |rid, code| {
        info!("Run {rid} exited with code {code:?}");
        discovery.cancel(rid);
    });

    // Taken before spawn so nothing the process creates can predate it.
    let spawned_at = SystemTime::now();
    state
        .pty
        .spawn(
            run_id.clone(),
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

    match &resume_token {
        Some(token) => state.resume_discovery.claim(token),
        None if matches!(
            provider,
            ProviderId::Codex | ProviderId::Antigravity | ProviderId::Omp
        ) =>
        {
            state.resume_discovery.track(PendingRun {
                run_id,
                provider,
                cwd,
                spawned_at,
                events,
            });
        }
        None => {}
    }

    Ok(SessionStartInfo {
        resumed,
        resume_token: resume_token.or(session_app_id),
    })
}

/// Write input to a running PTY.
#[tauri::command]
pub async fn session_write(
    run_id: String,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.write(&run_id, &data).map_err(ApiError::Pty)
}

/// Resize a running PTY.
#[tauri::command]
pub async fn session_resize(
    run_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.pty.resize(&run_id, cols, rows).map_err(ApiError::Pty)
}

/// Stop a run. No-op when no PTY is live.
#[tauri::command]
pub async fn session_stop(
    run_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.resume_discovery.cancel(&run_id);
    let _ = state.pty.kill(&run_id);
    Ok(())
}
