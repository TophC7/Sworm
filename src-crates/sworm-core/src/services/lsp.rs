use crate::events::EventSink;
use crate::services::builtins::LoadedBuiltinLspServerDefinition;
use crate::services::env::EnvironmentService;
use crate::services::nix::NixService;
use sworm_protocol::lsp::{
    LspEvent, LspServerConnectionStatus, LspServerStatus, LspTransportTraceDirection,
};
use sworm_protocol::settings::{LspServerConfigRecord, LspTraceLevel};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

const LSP_MAX_HEADER_BYTES: usize = 64 * 1024;
const LSP_MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ProjectLspEnvironment {
    pub merged_path: String,
    pub child_env: HashMap<String, String>,
}

impl ProjectLspEnvironment {
    pub fn from_host(env: &EnvironmentService) -> Self {
        Self {
            merged_path: env.merged_path.clone(),
            child_env: env.child_env.clone(),
        }
    }

    pub fn from_nix(env: &EnvironmentService, nix_env: Option<&HashMap<String, String>>) -> Self {
        match nix_env {
            Some(nix_env) => Self {
                merged_path: NixService::merged_path(&env.merged_path, nix_env),
                child_env: NixService::merge_env(&env.child_env, nix_env),
            },
            None => Self::from_host(env),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedLspCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: HashMap<String, String>,
    pub resolved_path: Option<String>,
    pub runtime_resolved_path: Option<String>,
}

pub struct LspService {
    sessions: Arc<Mutex<HashMap<String, LiveLspSession>>>,
}

struct LiveLspSession {
    stdin: std::process::ChildStdin,
    child: Arc<Mutex<Child>>,
    shutdown: Arc<AtomicBool>,
    finalized: Arc<AtomicBool>,
    runtime_id: String,
    trace: LspTraceLevel,
    events: EventSink<LspEvent>,
    owner_id: Option<String>,
}

impl LspService {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn spawn(
        &self,
        session_id: String,
        owner_id: Option<String>,
        trace: LspTraceLevel,
        resolved: ResolvedLspCommand,
        events: EventSink<LspEvent>,
    ) -> Result<(), String> {
        if self.is_alive(&session_id) {
            let _ = self.kill(&session_id);
        }

        let mut command = Command::new(&resolved.program);
        command.args(&resolved.args);
        command.current_dir(&resolved.cwd);
        command.env_clear();
        command.envs(&resolved.env);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let mut child = command
            .spawn()
            .map_err(|error| format!("Failed to spawn LSP process: {}", error))?;
        let pid = child.id();
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "LSP stdout pipe missing".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "LSP stderr pipe missing".to_string())?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "LSP stdin pipe missing".to_string())?;

        let child = Arc::new(Mutex::new(child));
        let shutdown = Arc::new(AtomicBool::new(false));
        let finalized = Arc::new(AtomicBool::new(false));
        let runtime_id = uuid::Uuid::new_v4().to_string();

        self.sessions.lock().insert(
            session_id.clone(),
            LiveLspSession {
                stdin,
                child: Arc::clone(&child),
                shutdown: Arc::clone(&shutdown),
                finalized: Arc::clone(&finalized),
                runtime_id: runtime_id.clone(),
                trace,
                events: Arc::clone(&events),
                owner_id,
            },
        );

        let _ = (events)(LspEvent::Started {
            session_id: session_id.clone(),
            pid: Some(pid),
            resolved_path: resolved.resolved_path.clone(),
            runtime_resolved_path: resolved.runtime_resolved_path.clone(),
        });

        self.spawn_stdout_thread(
            session_id.clone(),
            runtime_id.clone(),
            stdout,
            Arc::clone(&child),
            trace,
            Arc::clone(&events),
            Arc::clone(&shutdown),
        );
        self.spawn_stderr_thread(
            session_id.clone(),
            runtime_id.clone(),
            stderr,
            trace,
            Arc::clone(&events),
            Arc::clone(&shutdown),
        );
        self.spawn_wait_thread(session_id, runtime_id, child, events, shutdown, finalized);

        Ok(())
    }

    pub fn send(&self, session_id: &str, message_json: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock();
        let live = sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("No active LSP session: {}", session_id))?;

        let payload = format!(
            "Content-Length: {}\r\n\r\n{}",
            message_json.len(),
            message_json
        );

        live.stdin
            .write_all(payload.as_bytes())
            .map_err(|error| format!("Failed to write LSP message: {}", error))?;
        live.stdin
            .flush()
            .map_err(|error| format!("Failed to flush LSP stdin: {}", error))?;

        if live.trace != LspTraceLevel::Off {
            let _ = (live.events)(LspEvent::Trace {
                session_id: session_id.to_string(),
                direction: LspTransportTraceDirection::Outgoing,
                payload: message_json.to_string(),
            });
        }

        Ok(())
    }

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let live = self.sessions.lock().remove(session_id);
        if let Some(live) = live {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            live.child
                .lock()
                .kill()
                .map_err(|error| format!("Failed to kill LSP process: {}", error))?;
            Ok(())
        } else {
            Err(format!("No active LSP session: {}", session_id))
        }
    }
    pub fn kill_owner(&self, owner_id: &str) -> usize {
        let live_sessions: Vec<(String, LiveLspSession)> = {
            let mut sessions = self.sessions.lock();
            let session_ids: Vec<String> = sessions
                .iter()
                .filter(|(_, live)| live.owner_id.as_deref() == Some(owner_id))
                .map(|(session_id, _)| session_id.clone())
                .collect();
            session_ids
                .into_iter()
                .filter_map(|session_id| {
                    sessions.remove(&session_id).map(|live| (session_id, live))
                })
                .collect()
        };
        let total = live_sessions.len();
        for (session_id, live) in live_sessions {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if let Err(error) = live.child.lock().kill() {
                warn!("Failed to kill LSP process {}: {}", session_id, error);
            }
        }
        total
    }

    pub fn kill_all(&self) -> usize {
        let live_sessions: Vec<(String, LiveLspSession)> = self.sessions.lock().drain().collect();
        let total = live_sessions.len();
        for (session_id, live) in live_sessions {
            live.shutdown.store(true, Ordering::Relaxed);
            live.finalized.store(true, Ordering::Release);
            if let Err(error) = live.child.lock().kill() {
                warn!("Failed to kill LSP process {}: {}", session_id, error);
            }
        }
        total
    }

    pub fn is_alive(&self, session_id: &str) -> bool {
        self.sessions.lock().contains_key(session_id)
    }

    fn spawn_stdout_thread(
        &self,
        session_id: String,
        runtime_id: String,
        stdout: ChildStdout,
        child: Arc<Mutex<Child>>,
        trace: LspTraceLevel,
        events: EventSink<LspEvent>,
        shutdown: Arc<AtomicBool>,
    ) {
        std::thread::Builder::new()
            .name(format!("lsp-stdout-{}", session_id))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    match read_lsp_message(&mut reader) {
                        Ok(Some(message)) => {
                            if trace != LspTraceLevel::Off {
                                let _ = (events)(LspEvent::Trace {
                                    session_id: session_id.clone(),
                                    direction: LspTransportTraceDirection::Incoming,
                                    payload: message.clone(),
                                });
                            }
                            let _ = (events)(LspEvent::Message {
                                session_id: session_id.clone(),
                                payload_json: message,
                            });
                        }
                        Ok(None) => break,
                        Err(error) => {
                            if !shutdown.load(Ordering::Relaxed) {
                                let _ = (events)(LspEvent::Error {
                                    session_id: session_id.clone(),
                                    message: error,
                                });
                                if let Err(kill_error) = child.lock().kill() {
                                    warn!(
                                        "Failed to kill LSP process {} after framing error: {}",
                                        session_id, kill_error
                                    );
                                }
                            }
                            break;
                        }
                    }
                }
                info!(
                    "LSP stdout thread finished for {} ({})",
                    session_id, runtime_id
                );
            })
            .expect("failed to spawn LSP stdout thread");
    }

    fn spawn_stderr_thread(
        &self,
        session_id: String,
        runtime_id: String,
        stderr: ChildStderr,
        trace: LspTraceLevel,
        events: EventSink<LspEvent>,
        shutdown: Arc<AtomicBool>,
    ) {
        std::thread::Builder::new()
            .name(format!("lsp-stderr-{}", session_id))
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            let trimmed = line.trim_end().to_string();
                            if trimmed.is_empty() {
                                continue;
                            }
                            if trace != LspTraceLevel::Off {
                                let _ = (events)(LspEvent::Trace {
                                    session_id: session_id.clone(),
                                    direction: LspTransportTraceDirection::Stderr,
                                    payload: trimmed.clone(),
                                });
                            }
                            if trace == LspTraceLevel::Verbose {
                                info!("LSP stderr {}: {}", session_id, trimmed);
                            }
                        }
                        Err(error) => {
                            if !shutdown.load(Ordering::Relaxed) {
                                let _ = (events)(LspEvent::Error {
                                    session_id: session_id.clone(),
                                    message: format!("Failed reading LSP stderr: {}", error),
                                });
                            }
                            break;
                        }
                    }
                    if shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                }
                info!(
                    "LSP stderr thread finished for {} ({})",
                    session_id, runtime_id
                );
            })
            .expect("failed to spawn LSP stderr thread");
    }

    fn spawn_wait_thread(
        &self,
        session_id: String,
        runtime_id: String,
        child: Arc<Mutex<Child>>,
        events: EventSink<LspEvent>,
        shutdown: Arc<AtomicBool>,
        finalized: Arc<AtomicBool>,
    ) {
        let sessions = Arc::clone(&self.sessions);
        std::thread::Builder::new()
            .name(format!("lsp-wait-{}", session_id))
            .spawn(move || {
                let exit_code = loop {
                    match child.lock().try_wait() {
                        Ok(Some(status)) => break status.code(),
                        Ok(None) => {
                            thread::sleep(Duration::from_millis(50));
                        }
                        Err(error) => {
                            if !shutdown.load(Ordering::Relaxed) {
                                let _ = (events)(LspEvent::Error {
                                    session_id: session_id.clone(),
                                    message: format!("Failed waiting for LSP process: {}", error),
                                });
                            }
                            break None;
                        }
                    }
                };

                {
                    let mut sessions = sessions.lock();
                    let remove_current = sessions
                        .get(&session_id)
                        .map(|live| live.runtime_id == runtime_id)
                        .unwrap_or(false);
                    if remove_current {
                        sessions.remove(&session_id);
                    }
                }

                if finalized.swap(true, Ordering::AcqRel) {
                    return;
                }

                shutdown.store(true, Ordering::Relaxed);
                let _ = (events)(LspEvent::Exit {
                    session_id,
                    code: exit_code,
                });
            })
            .expect("failed to spawn LSP wait thread");
    }
}

pub fn resolve_server_status(
    server: &LoadedBuiltinLspServerDefinition,
    config: &LspServerConfigRecord,
    env: &ProjectLspEnvironment,
) -> LspServerStatus {
    if !config.enabled {
        return LspServerStatus {
            server_definition_id: server.server_definition_id.clone(),
            builtin_id: server.builtin_id.clone(),
            builtin_label: server.builtin_label.clone(),
            label: server.definition.label.clone(),
            enabled: false,
            status: LspServerConnectionStatus::Disabled,
            resolved_path: None,
            runtime_resolved_path: None,
            message: None,
            install_hint: server.definition.install_hint.clone(),
            document_selectors: server.definition.document_selectors.clone(),
            initialization_options: server.definition.initialization_options.clone(),
            settings: server.definition.settings.clone(),
        };
    }

    match resolve_launch(server, config, env, ".") {
        Ok(resolved) => LspServerStatus {
            server_definition_id: server.server_definition_id.clone(),
            builtin_id: server.builtin_id.clone(),
            builtin_label: server.builtin_label.clone(),
            label: server.definition.label.clone(),
            enabled: true,
            status: LspServerConnectionStatus::Connected,
            resolved_path: resolved.resolved_path,
            runtime_resolved_path: resolved.runtime_resolved_path,
            message: None,
            install_hint: server.definition.install_hint.clone(),
            document_selectors: server.definition.document_selectors.clone(),
            initialization_options: server.definition.initialization_options.clone(),
            settings: server.definition.settings.clone(),
        },
        Err(message) => LspServerStatus {
            server_definition_id: server.server_definition_id.clone(),
            builtin_id: server.builtin_id.clone(),
            builtin_label: server.builtin_label.clone(),
            label: server.definition.label.clone(),
            enabled: true,
            status: LspServerConnectionStatus::Missing,
            resolved_path: None,
            runtime_resolved_path: None,
            message: Some(message),
            install_hint: server.definition.install_hint.clone(),
            document_selectors: server.definition.document_selectors.clone(),
            initialization_options: server.definition.initialization_options.clone(),
            settings: server.definition.settings.clone(),
        },
    }
}

pub fn resolve_launch(
    server: &LoadedBuiltinLspServerDefinition,
    config: &LspServerConfigRecord,
    env: &ProjectLspEnvironment,
    cwd: &str,
) -> Result<ResolvedLspCommand, String> {
    let runtime = &server.definition.runtime;
    let extra_args = config.extra_args.clone();

    let program = resolve_host_command(
        &runtime.command,
        &env.merged_path,
        config.binary_path_override.as_deref(),
    )?;

    let mut args = server.definition.args.clone();
    args.extend(extra_args);

    Ok(ResolvedLspCommand {
        program: program.clone(),
        args,
        cwd: cwd.to_string(),
        env: env.child_env.clone(),
        resolved_path: Some(program),
        runtime_resolved_path: None,
    })
}

fn resolve_host_command(
    command: &str,
    merged_path: &str,
    override_path: Option<&str>,
) -> Result<String, String> {
    let candidate = override_path.unwrap_or(command).trim();
    if candidate.is_empty() {
        return Err("Command path is empty".to_string());
    }

    if candidate.contains('/') {
        return validate_direct_command_path(candidate);
    }

    which::which_in(candidate, Some(merged_path), ".")
        .map(|path| path.to_string_lossy().to_string())
        .map_err(|_| format!("{} is not installed or not on PATH", candidate))
}

fn validate_direct_command_path(candidate: &str) -> Result<String, String> {
    let path = Path::new(candidate);
    if !path.exists() {
        return Err(format!("Command not found at {}", candidate));
    }
    if !path.is_file() {
        return Err(format!("Command path is not a file: {}", candidate));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = path
            .metadata()
            .map_err(|error| format!("Failed to read command metadata {}: {}", candidate, error))?;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(format!("Command is not executable: {}", candidate));
        }
    }

    Ok(candidate.to_string())
}

fn read_lsp_message<R: Read>(reader: &mut BufReader<R>) -> Result<Option<String>, String> {
    let mut content_length = None;
    let mut header_bytes = 0;
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader
            .by_ref()
            .take((LSP_MAX_HEADER_BYTES - header_bytes + 1) as u64)
            .read_line(&mut line)
            .map_err(|error| format!("Failed to read LSP headers: {}", error))?;
        header_bytes += bytes;
        if header_bytes > LSP_MAX_HEADER_BYTES {
            return Err(format!(
                "LSP header block exceeds {} bytes",
                LSP_MAX_HEADER_BYTES
            ));
        }
        if bytes == 0 {
            return if header_bytes == 0 {
                Ok(None)
            } else {
                Err("Unexpected EOF inside LSP headers".to_string())
            };
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("content-length") {
            continue;
        }
        if content_length.is_some() {
            return Err("Duplicate Content-Length header in LSP message".to_string());
        }
        content_length = Some(
            value
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("Invalid Content-Length header: {}", value.trim()))?,
        );
    }

    let content_length =
        content_length.ok_or_else(|| "Missing Content-Length header in LSP message".to_string())?;
    if content_length > LSP_MAX_PAYLOAD_BYTES {
        return Err(format!(
            "LSP payload of {} bytes exceeds limit of {} bytes",
            content_length, LSP_MAX_PAYLOAD_BYTES
        ));
    }
    let mut body = vec![0_u8; content_length];
    reader
        .read_exact(&mut body)
        .map_err(|error| format!("Failed to read LSP payload: {}", error))?;

    String::from_utf8(body)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_framed_lsp_messages() {
        let payload = r#"{"jsonrpc":"2.0","method":"initialized"}"#;
        let framed = format!("Content-Length: {}\r\n\r\n{}", payload.len(), payload);
        let mut reader = BufReader::new(framed.as_bytes());

        let message = read_lsp_message(&mut reader).unwrap();
        assert_eq!(message.as_deref(), Some(payload));
    }

    #[test]
    fn rejects_oversized_payload() {
        let framed = format!("Content-Length: {}\r\n\r\n", LSP_MAX_PAYLOAD_BYTES + 1);
        let error = read_lsp_message(&mut BufReader::new(framed.as_bytes())).unwrap_err();
        assert!(error.contains("exceeds limit"));
    }

    #[test]
    fn rejects_oversized_header_block() {
        let framed = "x".repeat(LSP_MAX_HEADER_BYTES + 16);
        let error = read_lsp_message(&mut BufReader::new(framed.as_bytes())).unwrap_err();
        assert!(error.contains("header block exceeds"));
    }

    #[test]
    fn rejects_duplicate_content_length() {
        let framed = b"Content-Length: 2\r\nContent-Length: 3\r\n\r\n{}";
        let error = read_lsp_message(&mut BufReader::new(framed.as_slice())).unwrap_err();
        assert!(error.contains("Duplicate"));
    }

    #[test]
    fn accepts_case_insensitive_content_length() {
        let framed = b"content-length: 2\r\ncontent-type: application/vscode-jsonrpc\r\n\r\n{}";
        let message = read_lsp_message(&mut BufReader::new(framed.as_slice())).unwrap();
        assert_eq!(message.as_deref(), Some("{}"));
    }

    #[test]
    fn rejects_missing_direct_command_paths() {
        let missing =
            std::env::temp_dir().join(format!("sworm-lsp-missing-{}", uuid::Uuid::new_v4()));
        let result = validate_direct_command_path(missing.to_string_lossy().as_ref());
        assert!(result.is_err());
    }

    #[cfg(unix)]
    #[test]
    fn accepts_executable_direct_command_paths() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("sworm-lsp-command-{}", uuid::Uuid::new_v4()));
        fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();

        let resolved = validate_direct_command_path(path.to_string_lossy().as_ref()).unwrap();
        assert_eq!(resolved, path.to_string_lossy());

        fs::remove_file(path).unwrap();
    }
}
