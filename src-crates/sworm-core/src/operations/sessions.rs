use crate::errors::ApiError;
use crate::host::Host;
use crate::services::folders::resolve_folder;
use crate::services::omp;
use crate::services::providers::{
    binary_override, discovers_resume_token, resume_token_exists, ProviderService,
};
use crate::services::pty::{CompletedRunSink, PtySubscriber};
use crate::services::resume_discovery::PendingRun;
use crate::services::runs::RunKind;
use crate::services::settings_resolution::{
    provider_config_record, resolve_effective_settings_for_folder_path,
};
use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::SystemTime;
use sworm_protocol::provider::ProviderId;
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
    pub fn session_start(
        &self,
        run_id: String,
        folder_path: String,
        provider_id: String,
        resume_token: Option<String>,
        cols: u16,
        rows: u16,
        subscriber: Option<PtySubscriber>,
        owner_id: Option<String>,
    ) -> Result<SessionStartInfo, ApiError> {
        let _owner = self.runs.owner_activity(owner_id.as_deref())?;
        let folder = resolve_folder(&folder_path)?;
        let kind = RunKind::Session {
            provider_id: provider_id.clone(),
        };
        self.runs.with_start(&run_id, |runs| {
            if runs.reuse(
                &run_id,
                &folder,
                &kind,
                &self.pty,
                subscriber.as_ref(),
                owner_id.as_deref(),
                cols,
                rows,
            )? {
                return Ok(SessionStartInfo {
                    resumed: true,
                    resume_token: self.runs.resume_token(&run_id),
                });
            }
            let (token, completed) = runs.reserve(&run_id, folder.clone(), kind);
            let token = token.expect("session reservation has token cell");
            let result = self.spawn_session(
                run_id.clone(),
                &folder,
                provider_id,
                resume_token,
                cols,
                rows,
                subscriber,
                owner_id,
                token.clone(),
                completed,
            );
            match result {
                Ok(info) => {
                    let mut current = token.lock();
                    if current.is_none() {
                        *current = info.resume_token.clone();
                    }
                    Ok(info)
                }
                Err(error) => {
                    runs.abort(&run_id);
                    Err(error)
                }
            }
        })
    }

    fn spawn_session(
        &self,
        run_id: String,
        folder: &std::path::Path,
        provider_id: String,
        resume_token: Option<String>,
        cols: u16,
        rows: u16,
        subscriber: Option<PtySubscriber>,
        owner_id: Option<String>,
        token: Arc<Mutex<Option<String>>>,
        completed: Option<CompletedRunSink>,
    ) -> Result<SessionStartInfo, ApiError> {
        let definition = ProviderService::definition(&provider_id).ok_or_else(|| {
            ApiError::InvalidArgument(format!("Unsupported provider: {provider_id}"))
        })?;
        let provider = definition.id;
        let effective_settings =
            resolve_effective_settings_for_folder_path(Some(folder)).map_err(ApiError::Internal)?;
        let provider_config = provider_config_record(&effective_settings.settings, &provider_id);
        if !provider_config.enabled {
            return Err(ApiError::InvalidArgument(format!(
                "Provider disabled by settings: {provider_id}"
            )));
        }
        let cwd = folder.to_string_lossy().into_owned();

        let mut child_env = self.folder_env(folder);

        let cli_cmd = if provider == ProviderId::Terminal {
            binary_override(provider_config.binary_path_override.as_deref())
                .map(str::to_string)
                .unwrap_or_else(|| self.env.detected_shell.clone())
        } else {
            ProviderService::resolve_command_path(
                &provider_id,
                &child_env["PATH"],
                provider_config.binary_path_override.as_deref(),
            )
            .expect("provider definition already validated")
        };

        let resume_token = validated_token(resume_token, definition.label, |token| {
            resume_token_exists(provider, &cwd, token)
        });
        let session_app_id = (provider == ProviderId::ClaudeCode && resume_token.is_none())
            .then(|| uuid::Uuid::new_v4().to_string());
        let resumed = resume_token.is_some();

        let mut args = ProviderService::build_start_args(
            &provider_id,
            resume_token.as_deref(),
            session_app_id.as_deref(),
        );
        args.extend(provider_config.extra_args);
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        // Bridge values override inherited and Nix-provided values.
        if provider != ProviderId::Terminal {
            match self.issue_bridge.ensure_running(folder) {
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
        let awaiting_token = resume_token.is_none() && discovers_resume_token(provider);
        let exited = awaiting_token.then(|| Arc::new(AtomicBool::new(false)));
        let exit_flag = exited.clone();
        let on_exit: Box<dyn FnOnce(&str, Option<i32>) + Send> = Box::new(move |rid, code| {
            info!("Run {rid} exited with code {code:?}");
            if let Some(exit_flag) = exit_flag {
                exit_flag.store(true, Ordering::Release);
            }
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
                subscriber,
                owner_id,
                Some(on_exit),
                completed,
            )
            .map_err(ApiError::Pty)?;

        match &resume_token {
            Some(token) => self.resume_discovery.claim(token),
            None if discovers_resume_token(provider) => {
                let tracked_run_id = run_id.clone();
                self.resume_discovery.track(PendingRun {
                    run_id,
                    provider,
                    cwd,
                    spawned_at,
                    event_sink,
                    on_bound: Box::new(move |bound| *token.lock() = Some(bound)),
                });
                // Exit can beat registration; its callback then had no pending
                // run to cancel. Check the same flag after registering.
                if exited
                    .expect("discoverable run has exit flag")
                    .load(Ordering::Acquire)
                {
                    self.resume_discovery.cancel(&tracked_run_id);
                }
            }
            None => {}
        }

        Ok(SessionStartInfo {
            resumed,
            resume_token: resume_token.or(session_app_id),
        })
    }

    /// Stop a session and release its retained record and transcript, rejecting task IDs.
    pub fn session_stop(&self, run_id: String) -> Result<(), ApiError> {
        self.runs.with_run(&run_id, |runs| {
            runs.stop(
                &run_id,
                RunKind::Session {
                    provider_id: String::new(),
                },
                &self.pty,
            )?;
            self.forget_run(&run_id);
            Ok(())
        })
    }

    pub fn omp_resolve_uri(
        &self,
        uri: String,
        cwd: Option<String>,
    ) -> Result<sworm_protocol::omp::OmpResolvedTarget, ApiError> {
        omp::resolve_omp_target(&uri, cwd.as_deref())
    }
}
