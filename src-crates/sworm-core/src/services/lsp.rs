use crate::errors::ApiError;
use crate::events::EventSink;
use crate::services::builtins::LoadedBuiltinLspServerDefinition;
#[cfg(unix)]
use crate::services::pty::{groups_terminated, stop_process};
use crate::services::pty::{ChildExit, PROCESS_STOP_TIMEOUT};
use sworm_protocol::lsp::{
    LspEvent, LspServerConnectionStatus, LspServerStatus, LspTransportTraceDirection,
};
use sworm_protocol::settings::{LspServerConfigRecord, LspTraceLevel};

use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

const LSP_MAX_HEADER_BYTES: usize = 64 * 1024;
const LSP_MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ResolvedLspCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: HashMap<String, String>,
}

pub struct LspService {
    registry: Arc<Mutex<LspRegistry>>,
}

#[derive(Default)]
struct LspRegistry {
    sessions: HashMap<String, LiveLspSession>,
    pending: HashMap<String, Arc<LspProcess>>,
}

struct LiveLspSession {
    stdin: std::process::ChildStdin,
    process: Arc<LspProcess>,
    trace: LspTraceLevel,
    owner_id: Option<String>,
}

struct LspProcess {
    session_id: String,
    runtime_id: String,
    project_root: PathBuf,
    pid: u32,
    child: Mutex<Child>,
    exit: ChildExit,
    shutdown: AtomicBool,
    finalized: AtomicBool,
    events: EventSink<LspEvent>,
    #[cfg(unix)]
    group: Mutex<Option<libc::pid_t>>,
}

impl LspProcess {
    fn poll_exit(&self) {
        // Signals take this same latch: a reaped numeric PID is never killed.
        let mut result = self.exit.result.lock();
        if result.is_some() {
            return;
        }
        *result = match self.child.lock().try_wait() {
            Ok(Some(status)) => Some(Ok(status.code())),
            Ok(None) => None,
            Err(error) => Some(Err(format!("Failed waiting for LSP process: {error}"))),
        };
        if result.is_some() {
            self.exit.changed.notify_all();
        }
    }

    fn suppress_exit(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
        self.finalized.store(true, Ordering::Release);
    }

    // Ordinary owner/stream teardown keeps its existing kill-and-remove policy.
    // The independent pending record still fences later destructive operations.
    fn signal_kill(&self) -> io::Result<()> {
        self.suppress_exit();
        let result = self.exit.result.lock();
        if matches!(result.as_ref(), Some(Ok(_))) {
            return Ok(());
        }
        if let Some(Err(error)) = result.as_ref() {
            return Err(io::Error::other(error.clone()));
        }
        self.child.lock().kill()
    }

    fn stop(&self, deadline: Instant) -> Result<(), String> {
        self.shutdown.store(true, Ordering::Relaxed);
        #[cfg(unix)]
        {
            // Serializes concurrent stops and clears a proven-dead group before
            // another stop can reuse its numeric identity.
            let mut group = self.group.lock();
            stop_process(
                Some(self.pid),
                group.as_slice(),
                &self.exit,
                deadline,
                || {
                    self.poll_exit();
                },
            )?;
            *group = None;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            self.poll_exit();
            if self.exit.result().is_none() {
                self.signal_kill().map_err(|error| error.to_string())?;
            }
            loop {
                self.poll_exit();
                if let Some(result) = self.exit.result() {
                    return result.map(|_| ());
                }
                if Instant::now() >= deadline {
                    return Err(format!("Timed out waiting for LSP process {}", self.pid));
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    }

    fn fully_exited(&self) -> bool {
        if !matches!(self.exit.result(), Some(Ok(_))) {
            return false;
        }
        #[cfg(unix)]
        {
            let mut group = self.group.lock();
            if groups_terminated(group.as_slice()).unwrap_or(false) {
                *group = None;
                true
            } else {
                false
            }
        }
        #[cfg(not(unix))]
        true
    }
}

impl LspService {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Mutex::new(LspRegistry::default())),
        }
    }

    pub fn spawn(
        &self,
        session_id: String,
        project_root: &Path,
        owner_id: Option<String>,
        trace: LspTraceLevel,
        resolved: ResolvedLspCommand,
        events: EventSink<LspEvent>,
    ) -> Result<(), ApiError> {
        // The invoking project, not the server's nested cwd/root_path, owns it.
        let project_root = project_root.canonicalize().map_err(ApiError::from)?;
        // The lease holder decides when its server dies, never a silent restart
        // of someone else's session.
        if self.is_alive(&session_id) {
            return Err(already_active(&session_id));
        }

        let mut command = Command::new(&resolved.program);
        command.args(&resolved.args);
        command.current_dir(&resolved.cwd);
        command.env_clear();
        command.envs(&resolved.env);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command
            .spawn()
            .map_err(|error| ApiError::Internal(format!("Failed to spawn LSP process: {error}")))?;
        let pid = child.id();
        let pipes = (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let process = Arc::new(LspProcess {
            session_id: session_id.clone(),
            runtime_id: uuid::Uuid::new_v4().to_string(),
            project_root,
            pid,
            child: Mutex::new(child),
            exit: ChildExit::default(),
            shutdown: AtomicBool::new(false),
            finalized: AtomicBool::new(false),
            events: Arc::clone(&events),
            #[cfg(unix)]
            group: Mutex::new(Some(libc::pid_t::try_from(pid).unwrap_or(0))),
        });

        let (stdout, stderr) = {
            let mut registry = self.registry.lock();
            // Register even rejected or incompletely started incarnations:
            // failed cleanup must remain visible to the destructive barrier.
            registry
                .pending
                .insert(process.runtime_id.clone(), Arc::clone(&process));
            if registry.sessions.contains_key(&session_id) {
                drop(registry);
                return Err(self.startup_failure(&process, already_active(&session_id)));
            }
            let (Some(stdin), Some(stdout), Some(stderr)) = pipes else {
                drop(registry);
                return Err(self.startup_failure(
                    &process,
                    ApiError::Internal("LSP process pipe missing".to_owned()),
                ));
            };
            registry.sessions.insert(
                session_id.clone(),
                LiveLspSession {
                    stdin,
                    process: Arc::clone(&process),
                    trace,
                    owner_id,
                },
            );
            (stdout, stderr)
        };

        let _ = (events)(LspEvent::Started {
            session_id,
            pid: Some(pid),
            resolved_path: Some(resolved.program),
        });

        let startup = self
            .spawn_stdout_thread(Arc::clone(&process), stdout, trace)
            .and_then(|_| self.spawn_stderr_thread(Arc::clone(&process), stderr, trace))
            .and_then(|_| self.spawn_wait_thread(Arc::clone(&process)));
        if let Err(error) = startup {
            return Err(self.startup_failure(
                &process,
                ApiError::Internal(format!("Failed to spawn LSP reader/wait thread: {error}")),
            ));
        }
        Ok(())
    }

    fn startup_failure(&self, process: &Arc<LspProcess>, error: ApiError) -> ApiError {
        {
            let mut registry = self.registry.lock();
            if registry
                .sessions
                .get(&process.session_id)
                .is_some_and(|live| live.process.runtime_id == process.runtime_id)
            {
                registry.sessions.remove(&process.session_id);
            }
        }
        process.suppress_exit();
        match process.stop(Instant::now() + PROCESS_STOP_TIMEOUT) {
            Ok(()) => {
                self.registry.lock().pending.remove(&process.runtime_id);
                error
            }
            Err(stop_error) => ApiError::Internal(format!(
                "{error}; failed to stop incompletely started LSP process: {stop_error}"
            )),
        }
    }

    pub fn send(&self, session_id: &str, message_json: &str) -> Result<(), String> {
        let mut registry = self.registry.lock();
        let live = registry
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| format!("No active LSP session: {session_id}"))?;
        let payload = format!(
            "Content-Length: {}\r\n\r\n{}",
            message_json.len(),
            message_json
        );
        live.stdin
            .write_all(payload.as_bytes())
            .map_err(|error| format!("Failed to write LSP message: {error}"))?;
        live.stdin
            .flush()
            .map_err(|error| format!("Failed to flush LSP stdin: {error}"))?;
        if live.trace != LspTraceLevel::Off {
            let _ = (live.process.events)(LspEvent::Trace {
                session_id: session_id.to_string(),
                direction: LspTransportTraceDirection::Outgoing,
                payload: message_json.to_string(),
            });
        }
        Ok(())
    }

    pub fn kill(&self, session_id: &str) -> Result<(), String> {
        let processes = self.retire_matching(|process| process.session_id == session_id);
        if processes.is_empty() {
            return Err(format!("No active LSP session: {session_id}"));
        }
        self.stop_processes(processes, Instant::now() + PROCESS_STOP_TIMEOUT)
    }

    /// Called under Host's exclusive admission with the shared PTY/LSP deadline.
    /// Removed and already-stopping sessions remain in the pending registry.
    pub fn drain_roots(&self, roots: &[PathBuf], deadline: Instant) -> Result<(), String> {
        let processes = self.retire_matching(|process| {
            roots
                .iter()
                .any(|root| process.project_root.starts_with(root))
        });
        self.stop_processes(processes, deadline)
    }

    fn retire_matching(&self, matches: impl Fn(&LspProcess) -> bool) -> Vec<Arc<LspProcess>> {
        let mut registry = self.registry.lock();
        let processes: Vec<_> = registry
            .pending
            .values()
            .filter(|process| matches(process))
            .cloned()
            .collect();
        for process in &processes {
            process.suppress_exit();
            if registry
                .sessions
                .get(&process.session_id)
                .is_some_and(|live| live.process.runtime_id == process.runtime_id)
            {
                registry.sessions.remove(&process.session_id);
            }
        }
        processes
    }

    fn stop_processes(
        &self,
        processes: Vec<Arc<LspProcess>>,
        deadline: Instant,
    ) -> Result<(), String> {
        let mut failures = Vec::new();
        for process in processes {
            match process.stop(deadline) {
                Ok(()) => {
                    self.registry.lock().pending.remove(&process.runtime_id);
                }
                Err(error) => failures.push(format!("{}: {error}", process.session_id)),
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Failed to stop LSP processes: {}",
                failures.join("; ")
            ))
        }
    }

    pub fn kill_owner(&self, owner_id: &str) -> usize {
        let live_sessions: Vec<_> = {
            let mut registry = self.registry.lock();
            let ids: Vec<_> = registry
                .sessions
                .iter()
                .filter(|(_, live)| live.owner_id.as_deref() == Some(owner_id))
                .map(|(id, _)| id.clone())
                .collect();
            ids.into_iter()
                .filter_map(|id| registry.sessions.remove(&id))
                .collect()
        };
        let total = live_sessions.len();
        for live in live_sessions {
            if let Err(error) = live.process.signal_kill() {
                warn!(
                    "Failed to kill LSP process {}: {error}",
                    live.process.session_id
                );
            }
        }
        total
    }

    pub fn kill_all(&self) -> usize {
        let live_sessions: Vec<_> = self
            .registry
            .lock()
            .sessions
            .drain()
            .map(|(_, live)| live)
            .collect();
        let total = live_sessions.len();
        for live in live_sessions {
            if let Err(error) = live.process.signal_kill() {
                warn!(
                    "Failed to kill LSP process {}: {error}",
                    live.process.session_id
                );
            }
        }
        total
    }

    pub fn is_alive(&self, session_id: &str) -> bool {
        self.registry.lock().sessions.contains_key(session_id)
    }

    fn spawn_stdout_thread(
        &self,
        process: Arc<LspProcess>,
        stdout: ChildStdout,
        trace: LspTraceLevel,
    ) -> io::Result<()> {
        thread::Builder::new()
            .name(format!("lsp-stdout-{}", process.session_id))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    if process.shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                    match read_lsp_message(&mut reader) {
                        Ok(Some(message)) => {
                            if trace != LspTraceLevel::Off {
                                let _ = (process.events)(LspEvent::Trace {
                                    session_id: process.session_id.clone(),
                                    direction: LspTransportTraceDirection::Incoming,
                                    payload: message.clone(),
                                });
                            }
                            let _ = (process.events)(LspEvent::Message {
                                session_id: process.session_id.clone(),
                                payload_json: message,
                            });
                        }
                        Ok(None) => break,
                        Err(error) => {
                            if !process.shutdown.load(Ordering::Relaxed) {
                                let _ = (process.events)(LspEvent::Error {
                                    session_id: process.session_id.clone(),
                                    message: error,
                                });
                                if let Err(error) = process.stop(Instant::now() + PROCESS_STOP_TIMEOUT) {
                                    warn!("Failed to stop LSP process {} after framing error: {error}", process.session_id);
                                }
                            }
                            break;
                        }
                    }
                }
                info!("LSP stdout thread finished for {} ({})", process.session_id, process.runtime_id);
            })
            .map(|_| ())
    }

    fn spawn_stderr_thread(
        &self,
        process: Arc<LspProcess>,
        stderr: ChildStderr,
        trace: LspTraceLevel,
    ) -> io::Result<()> {
        thread::Builder::new()
            .name(format!("lsp-stderr-{}", process.session_id))
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
                                let _ = (process.events)(LspEvent::Trace {
                                    session_id: process.session_id.clone(),
                                    direction: LspTransportTraceDirection::Stderr,
                                    payload: trimmed.clone(),
                                });
                            }
                            if trace == LspTraceLevel::Verbose {
                                info!("LSP stderr {}: {trimmed}", process.session_id);
                            }
                        }
                        Err(error) => {
                            if !process.shutdown.load(Ordering::Relaxed) {
                                let _ = (process.events)(LspEvent::Error {
                                    session_id: process.session_id.clone(),
                                    message: format!("Failed reading LSP stderr: {error}"),
                                });
                            }
                            break;
                        }
                    }
                    if process.shutdown.load(Ordering::Relaxed) {
                        break;
                    }
                }
                info!(
                    "LSP stderr thread finished for {} ({})",
                    process.session_id, process.runtime_id
                );
            })
            .map(|_| ())
    }

    fn spawn_wait_thread(&self, process: Arc<LspProcess>) -> io::Result<()> {
        let registry = Arc::clone(&self.registry);
        thread::Builder::new()
            .name(format!("lsp-wait-{}", process.session_id))
            .spawn(move || {
                let result = loop {
                    process.poll_exit();
                    if let Some(result) = process.exit.result() {
                        break result;
                    }
                    thread::sleep(Duration::from_millis(50));
                };
                // The independent OS acknowledgement is already published,
                // before registry removal, reader shutdown or event suppression.
                {
                    let mut registry = registry.lock();
                    if registry
                        .sessions
                        .get(&process.session_id)
                        .is_some_and(|live| live.process.runtime_id == process.runtime_id)
                    {
                        registry.sessions.remove(&process.session_id);
                    }
                }
                let wait_succeeded = result.is_ok();
                if !process.finalized.swap(true, Ordering::AcqRel) {
                    process.shutdown.store(true, Ordering::Relaxed);
                    let code = match result {
                        Ok(code) => code,
                        Err(error) => {
                            let _ = (process.events)(LspEvent::Error {
                                session_id: process.session_id.clone(),
                                message: error,
                            });
                            None
                        }
                    };
                    let _ = (process.events)(LspEvent::Exit {
                        session_id: process.session_id.clone(),
                        code,
                    });
                }
                if wait_succeeded {
                    // A descendant can outlive the leader and retain its pipes.
                    // Retire the numeric group as soon as its last member exits,
                    // even when no subsequent stop/drain is requested.
                    while !process.fully_exited() {
                        thread::sleep(Duration::from_millis(50));
                    }
                    registry.lock().pending.remove(&process.runtime_id);
                }
            })
            .map(|_| ())
    }
}

fn already_active(session_id: &str) -> ApiError {
    ApiError::LspAlreadyActive {
        session_id: session_id.to_owned(),
    }
}

pub fn resolve_server_status(
    server: &LoadedBuiltinLspServerDefinition,
    config: &LspServerConfigRecord,
    env: &HashMap<String, String>,
) -> LspServerStatus {
    let (status, resolved_path, message) = if !config.enabled {
        (LspServerConnectionStatus::Disabled, None, None)
    } else {
        match resolve_launch(server, config, env, ".") {
            Ok(resolved) => (
                LspServerConnectionStatus::Connected,
                Some(resolved.program),
                None,
            ),
            Err(message) => (LspServerConnectionStatus::Missing, None, Some(message)),
        }
    };

    LspServerStatus {
        server_definition_id: server.server_definition_id.clone(),
        builtin_id: server.builtin_id.clone(),
        builtin_label: server.builtin_label.clone(),
        label: server.definition.label.clone(),
        enabled: config.enabled,
        status,
        resolved_path,
        message,
        install_hint: server.definition.install_hint.clone(),
        document_selectors: server.definition.document_selectors.clone(),
        initialization_options: server.definition.initialization_options.clone(),
        settings: server.definition.settings.clone(),
    }
}

pub fn resolve_launch(
    server: &LoadedBuiltinLspServerDefinition,
    config: &LspServerConfigRecord,
    env: &HashMap<String, String>,
    cwd: &str,
) -> Result<ResolvedLspCommand, String> {
    let program = resolve_host_command(
        &server.definition.command,
        env.get("PATH").map(String::as_str).unwrap_or(""),
        config.binary_path_override.as_deref(),
    )?;

    let mut args = server.definition.args.clone();
    args.extend(config.extra_args.iter().cloned());

    Ok(ResolvedLspCommand {
        program,
        args,
        cwd: cwd.to_string(),
        env: env.clone(),
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

    #[cfg(unix)]
    mod processes {
        use super::*;

        struct ProcessTest {
            service: LspService,
            root: PathBuf,
        }

        impl ProcessTest {
            fn new() -> Self {
                let root =
                    std::env::temp_dir().join(format!("sworm-lsp-exit-{}", uuid::Uuid::new_v4()));
                fs::create_dir_all(&root).unwrap();
                Self {
                    service: LspService::new(),
                    root: root.canonicalize().unwrap(),
                }
            }

            fn folder(&self, name: &str) -> PathBuf {
                let folder = self.root.join(name);
                fs::create_dir_all(&folder).unwrap();
                folder
            }

            fn spawn(&self, id: &str, project: &Path, cwd: &Path, script: &str) -> Arc<LspProcess> {
                self.service
                    .spawn(
                        id.to_owned(),
                        project,
                        Some("test-owner".to_owned()),
                        LspTraceLevel::Off,
                        ResolvedLspCommand {
                            program: which::which("sh").unwrap().to_string_lossy().into_owned(),
                            args: vec!["-c".to_owned(), script.to_owned()],
                            cwd: cwd.to_string_lossy().into_owned(),
                            env: std::env::vars().collect(),
                        },
                        Arc::new(|_| Ok(())),
                    )
                    .unwrap();
                let process = Arc::clone(&self.service.registry.lock().sessions[id].process);
                wait_until(|| cwd.join("ready").exists());
                process
            }
        }

        impl Drop for ProcessTest {
            fn drop(&mut self) {
                let _ = self.service.drain_roots(
                    std::slice::from_ref(&self.root),
                    Instant::now() + PROCESS_STOP_TIMEOUT,
                );
                let _ = fs::remove_dir_all(&self.root);
            }
        }

        fn wait_until(mut condition: impl FnMut() -> bool) {
            let deadline = Instant::now() + PROCESS_STOP_TIMEOUT;
            while !condition() {
                assert!(
                    Instant::now() < deadline,
                    "LSP process did not reach expected state"
                );
                thread::sleep(Duration::from_millis(10));
            }
        }

        const SLEEP: &str = "printf ready > ready; exec sleep 30";
        const DESCENDANT: &str =
            "trap '' TERM; sleep 30 & printf '%s' \"$!\" > descendant; printf ready > ready; wait";

        #[test]
        fn strict_stop_reaps_leader_and_terminates_descendants() {
            let test = ProcessTest::new();
            let project = test.folder("project");
            let process = test.spawn("strict", &project, &project, DESCENDANT);
            let group = process.pid as libc::pid_t;
            assert_eq!(unsafe { libc::getpgid(group) }, group);
            assert_ne!(group, unsafe { libc::getpgrp() });
            let descendant = fs::read_to_string(project.join("descendant"))
                .unwrap()
                .parse::<libc::pid_t>()
                .unwrap();
            assert_eq!(unsafe { libc::kill(descendant, 0) }, 0);

            let started = Instant::now();
            test.service.kill("strict").unwrap();
            assert!(started.elapsed() < PROCESS_STOP_TIMEOUT);
            assert!(matches!(process.exit.result(), Some(Ok(_))));
            assert!(groups_terminated(&[group]).unwrap());
            assert!(!test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&process.runtime_id));
            assert!(project.is_dir());
        }

        #[test]
        fn drain_uses_canonical_invoking_project_not_server_cwd() {
            let test = ProcessTest::new();
            let target = test.folder("target");
            let other = test.folder("target-sibling");
            let alias = test.root.join("target-alias");
            std::os::unix::fs::symlink(&target, &alias).unwrap();
            let affected = test.spawn("affected", &alias, &other, SLEEP);
            let unaffected = test.spawn("unaffected", &other, &target, SLEEP);
            assert_eq!(affected.project_root, target);

            test.service
                .drain_roots(
                    std::slice::from_ref(&target),
                    Instant::now() + PROCESS_STOP_TIMEOUT,
                )
                .unwrap();
            assert!(matches!(affected.exit.result(), Some(Ok(_))));
            assert!(unaffected.exit.result().is_none());
            assert!(test.service.is_alive("unaffected"));
        }

        #[test]
        fn removed_incarnation_drain_keeps_replacement_project_alive() {
            let test = ProcessTest::new();
            let target = test.folder("target");
            let other = test.folder("other");
            let removed = test.spawn("same-id", &target, &target, DESCENDANT);
            let group = removed.pid as libc::pid_t;
            assert_eq!(test.service.kill_owner("test-owner"), 1);
            assert!(!test.service.is_alive("same-id"));
            wait_until(|| matches!(removed.exit.result(), Some(Ok(_))));
            assert!(!groups_terminated(&[group]).unwrap());
            assert!(test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&removed.runtime_id));
            let replacement = test.spawn("same-id", &other, &other, SLEEP);

            test.service
                .drain_roots(
                    std::slice::from_ref(&target),
                    Instant::now() + PROCESS_STOP_TIMEOUT,
                )
                .unwrap();
            assert!(groups_terminated(&[group]).unwrap());
            assert!(!test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&removed.runtime_id));
            assert!(replacement.exit.result().is_none());
            assert!(test.service.is_alive("same-id"));
        }

        #[test]
        fn reaped_group_is_retired_when_last_descendant_exits() {
            let test = ProcessTest::new();
            let project = test.folder("project");
            let process = test.spawn("group", &project, &project, DESCENDANT);
            let group = process.pid as libc::pid_t;
            assert_ne!(group, unsafe { libc::getpgrp() });
            assert_eq!(test.service.kill_owner("test-owner"), 1);
            wait_until(|| matches!(process.exit.result(), Some(Ok(_))));
            assert!(test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&process.runtime_id));
            assert_eq!(unsafe { libc::kill(-group, libc::SIGKILL) }, 0);
            wait_until(|| {
                !test
                    .service
                    .registry
                    .lock()
                    .pending
                    .contains_key(&process.runtime_id)
            });
            assert!(process.group.lock().is_none());
        }

        #[test]
        fn expired_drain_retains_pending_child_for_strict_retry() {
            let test = ProcessTest::new();
            let project = test.folder("project");
            let process = test.spawn("timeout", &project, &project, SLEEP);
            let error = test
                .service
                .drain_roots(std::slice::from_ref(&project), Instant::now())
                .unwrap_err();
            assert!(error.contains("Timed out"));
            assert!(project.is_dir());
            assert!(!test.service.is_alive("timeout"));
            assert!(process.exit.result().is_none());
            assert!(test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&process.runtime_id));
            assert_eq!(unsafe { libc::kill(process.pid as libc::pid_t, 0) }, 0);

            test.service.kill("timeout").unwrap();
            assert!(matches!(process.exit.result(), Some(Ok(_))));
            assert!(!test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&process.runtime_id));
        }

        #[test]
        fn actual_os_wait_failure_is_not_exit_proof_and_remains_pending() {
            let test = ProcessTest::new();
            let project = test.folder("project");
            let child = Command::new(which::which("sh").unwrap())
                .args(["-c", "exit 7"])
                .process_group(0)
                .spawn()
                .unwrap();
            let pid = child.id();
            let (events, received) = std::sync::mpsc::channel();
            let process = Arc::new(LspProcess {
                session_id: "wait-failure".to_owned(),
                runtime_id: uuid::Uuid::new_v4().to_string(),
                project_root: project.clone(),
                pid,
                child: Mutex::new(child),
                exit: ChildExit::default(),
                shutdown: AtomicBool::new(false),
                finalized: AtomicBool::new(false),
                events: Arc::new(move |event| {
                    events.send(event).map_err(|error| error.to_string())
                }),
                group: Mutex::new(Some(pid as libc::pid_t)),
            });
            test.service
                .registry
                .lock()
                .pending
                .insert(process.runtime_id.clone(), Arc::clone(&process));
            // Reap the real child outside std::process::Child to exercise ECHILD,
            // not a synthetic latch failure or a mocked process.
            let mut status = 0;
            assert_eq!(
                unsafe { libc::waitpid(pid as libc::pid_t, &mut status, 0) },
                pid as libc::pid_t
            );
            test.service
                .spawn_wait_thread(Arc::clone(&process))
                .unwrap();
            assert!(matches!(
                received.recv_timeout(PROCESS_STOP_TIMEOUT).unwrap(),
                LspEvent::Error { .. }
            ));
            assert!(matches!(
                received.recv_timeout(PROCESS_STOP_TIMEOUT).unwrap(),
                LspEvent::Exit { code: None, .. }
            ));
            assert!(matches!(process.exit.result(), Some(Err(_))));
            assert!(test
                .service
                .kill("wait-failure")
                .unwrap_err()
                .contains("Failed waiting"));
            assert!(test
                .service
                .drain_roots(
                    std::slice::from_ref(&project),
                    Instant::now() + PROCESS_STOP_TIMEOUT,
                )
                .is_err());
            assert!(project.is_dir());
            assert!(test
                .service
                .registry
                .lock()
                .pending
                .contains_key(&process.runtime_id));
        }
    }
}
