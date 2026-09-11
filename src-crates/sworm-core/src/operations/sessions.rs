use crate::errors::ApiError;
use crate::events::EventSink;
use crate::host::Host;
use crate::services::codex_state::CodexStateReader;
use crate::services::folders::resolve_folder;
use crate::services::nix::NixService;
use crate::services::omp;
use crate::services::providers::{
    antigravity_conversation_exists, claude_session_transcript_exists, ProviderService,
};
use crate::services::resume_discovery::PendingRun;
use crate::services::settings_resolution::{
    provider_config_record, resolve_effective_settings_for_folder_path,
};
use std::time::SystemTime;
use sworm_protocol::provider::ProviderId;
use sworm_protocol::pty::PtyEvent;
use sworm_protocol::session::SessionStartInfo;
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

impl Host {
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
    pub async fn session_start(
        &self,
        run_id: String,
        folder_path: String,
        provider_id: String,
        resume_token: Option<String>,
        cols: u16,
        rows: u16,
        output: EventSink<Vec<u8>>,
        events: EventSink<PtyEvent>,
        owner_id: Option<String>,
        window: bool,
    ) -> Result<SessionStartInfo, ApiError> {
        let provider = ProviderService::definition(&provider_id)
            .map(|definition| definition.id)
            .ok_or_else(|| {
                ApiError::InvalidArgument(format!("Unsupported provider: {provider_id}"))
            })?;
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
            let db = self.db.read();
            NixService::load_env_vars(db.conn(), &cwd).unwrap_or_else(|error| {
                warn!("Failed to load Nix env for folder {cwd}: {error}");
                None
            })
        };

        let effective_path = match &nix_env_vars {
            Some(nix_env) => NixService::merged_path(&self.env.merged_path, nix_env),
            None => self.env.merged_path.clone(),
        };

        let cli_cmd = if provider == ProviderId::Terminal {
            provider_config
                .binary_path_override
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| self.env.detected_shell.clone())
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

        let (resume_token, session_app_id) = match provider {
            ProviderId::ClaudeCode => {
                match validated_token(resume_token, "Claude session", |token| {
                    claude_session_transcript_exists(&cwd, token)
                }) {
                    Some(token) => (Some(token), None),
                    None => (None, Some(uuid::Uuid::new_v4().to_string())),
                }
            }
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
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        let mut child_env = match nix_env_vars {
            Some(nix_env) => NixService::merge_env(&self.env.child_env, &nix_env),
            None => self.env.child_env.clone(),
        };

        // Bridge values override inherited and Nix-provided values.
        if provider != ProviderId::Terminal {
            match self.issue_bridge.ensure_running(&folder) {
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

        let discovery = self.resume_discovery.clone();
        let on_exit: Box<dyn FnOnce(&str, Option<i32>) + Send> = Box::new(move |rid, code| {
            info!("Run {rid} exited with code {code:?}");
            discovery.cancel(rid);
        });

        // Taken before spawn so nothing the process creates can predate it.
        let spawned_at = SystemTime::now();
        let event_sink = self
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
                events,
                owner_id,
                window,
                Some(on_exit),
            )
            .map_err(ApiError::Pty)?;

        match &resume_token {
            Some(token) => self.resume_discovery.claim(token),
            None if matches!(
                provider,
                ProviderId::Codex | ProviderId::Antigravity | ProviderId::Omp
            ) =>
            {
                self.resume_discovery.track(PendingRun {
                    run_id,
                    provider,
                    cwd,
                    spawned_at,
                    event_sink,
                });
            }
            None => {}
        }

        Ok(SessionStartInfo {
            resumed,
            resume_token: resume_token.or(session_app_id),
        })
    }

    pub async fn session_write(&self, run_id: String, data: Vec<u8>) -> Result<(), ApiError> {
        self.pty.write(&run_id, &data).map_err(ApiError::Pty)
    }

    pub async fn session_resize(
        &self,
        run_id: String,
        cols: u16,
        rows: u16,
    ) -> Result<(), ApiError> {
        self.pty.resize(&run_id, cols, rows).map_err(ApiError::Pty)
    }

    /// Stop a run. No-op when no PTY is live.
    pub async fn session_stop(&self, run_id: String) -> Result<(), ApiError> {
        self.resume_discovery.cancel(&run_id);
        let _ = self.pty.kill(&run_id);
        Ok(())
    }

    pub async fn omp_resolve_uri(
        &self,
        uri: String,
        cwd: Option<String>,
    ) -> Result<sworm_protocol::omp::OmpResolvedTarget, ApiError> {
        tokio::task::spawn_blocking(move || omp::resolve_omp_target(&uri, cwd.as_deref()))
            .await
            .map_err(|error| ApiError::Internal(error.to_string()))?
    }
}
