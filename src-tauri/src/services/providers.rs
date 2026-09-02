use crate::models::provider::{ProviderConnectionStatus, ProviderId, ProviderStatus, ResumeMode};
use crate::services::folders::home_dir;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{info, warn};

/// Encode a working directory the same way Claude Code does for its
/// `~/.claude/projects/<dir>/<uuid>.jsonl` transcript layout: every `/`
/// and `.` becomes `-`. Hyphens pass through unchanged.
pub fn claude_project_dir_name(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c == '/' || c == '.' { '-' } else { c })
        .collect()
}

/// Path to the Claude Code transcript file for a given cwd + session UUID.
/// Returns None if `$HOME` is unset.
pub fn claude_transcript_path(cwd: &str, session_uuid: &str) -> Option<PathBuf> {
    let home = home_dir()?;
    let dir = claude_project_dir_name(cwd);
    Some(
        home.join(".claude")
            .join("projects")
            .join(dir)
            .join(format!("{}.jsonl", session_uuid)),
    )
}

/// Whether a Claude Code session transcript already exists on disk.
/// Used to choose between `--session-id` (new) and `--resume` (existing).
pub fn claude_session_transcript_exists(cwd: &str, session_uuid: &str) -> bool {
    claude_transcript_path(cwd, session_uuid)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// `~/.gemini/antigravity-cli` — the Antigravity CLI (`agy`) state root.
fn antigravity_dir() -> Option<PathBuf> {
    Some(home_dir()?.join(".gemini").join("antigravity-cli"))
}

/// Whether an Antigravity conversation store exists for `id`.
/// Used to choose between `--conversation <id>` and a fresh start.
pub fn antigravity_conversation_exists(id: &str) -> bool {
    antigravity_dir()
        .map(|dir| dir.join("conversations").join(format!("{id}.db")).exists())
        .unwrap_or(false)
}

/// Visit `conversations/*.db` files whose birth time is at or after
/// `since`, oldest first, excluding known ids. Returning `false` stops
/// iteration.
///
/// Fails closed: any error from `read_dir`, an entry, its metadata or
/// `created()` yields `None`, so a filesystem without birth times never
/// binds a conversation rather than binding the wrong one.
pub fn antigravity_visit_conversations_created_since(
    since: SystemTime,
    exclude: &HashSet<String>,
    mut visit: impl FnMut(SystemTime, String) -> bool,
) -> Option<()> {
    let entries = std::fs::read_dir(antigravity_dir()?.join("conversations")).ok()?;
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.ok()?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("db") {
            continue;
        }
        let created_at = entry.metadata().ok()?.created().ok()?;
        if created_at < since {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !exclude.contains(id) {
            candidates.push((created_at, id.to_string()));
        }
    }
    candidates.sort_by_key(|(created_at, _)| *created_at);
    for (created_at, id) in candidates {
        if !visit(created_at, id) {
            break;
        }
    }
    Some(())
}

/// Static provider definitions.
pub struct ProviderDef {
    pub id: ProviderId,
    pub label: &'static str,
    pub cli_command: &'static str,
    pub detect_commands: &'static [&'static str],
    pub version_args: &'static [&'static str],
    pub install_hint: &'static str,
    pub resume_mode: ResumeMode,
    pub default_args: &'static [&'static str],
}

const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        id: ProviderId::ClaudeCode,
        label: "Claude Code",
        cli_command: "claude",
        detect_commands: &["claude"],
        version_args: &["--version"],
        install_hint: "Install with: npm install -g @anthropic-ai/claude-code",
        resume_mode: ResumeMode::SessionId {
            session_flag: "--session-id",
            continue_flags: &["--resume"],
        },
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Codex,
        label: "Codex",
        cli_command: "codex",
        detect_commands: &["codex"],
        version_args: &["--version"],
        install_hint: "Install with: npm install -g @openai/codex",
        resume_mode: ResumeMode::ThreadId {
            resume_command: "resume",
        },
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Omp,
        label: "OMP",
        cli_command: "omp",
        detect_commands: &["omp"],
        version_args: &["--version"],
        install_hint: "Install OMP from your Nix/home-manager configuration or package manager.",
        resume_mode: ResumeMode::ThreadId {
            resume_command: "--resume",
        },
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Antigravity,
        label: "Antigravity",
        cli_command: "agy",
        detect_commands: &["agy"],
        version_args: &["--version"],
        install_hint: "Install the Antigravity CLI (agy).",
        resume_mode: ResumeMode::ConversationId {
            flag: "--conversation",
        },
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Terminal,
        label: "Terminal",
        cli_command: "sh",
        detect_commands: &[], // detected via $SHELL, not PATH lookup
        version_args: &["--version"],
        install_hint: "",
        resume_mode: ResumeMode::None,
        default_args: &[],
    },
];

/// Provider service: detection and static registry.
pub struct ProviderService;

impl ProviderService {
    pub fn detect_all(
        &self,
        merged_path: &str,
        binary_overrides: &HashMap<String, String>,
        detected_shell: Option<&str>,
    ) -> Vec<ProviderStatus> {
        let mut results = std::thread::scope(|scope| {
            let handles = PROVIDERS
                .iter()
                .map(|def| {
                    let override_path = binary_overrides.get(def.id.as_str()).map(String::as_str);
                    (
                        def,
                        scope.spawn(move || {
                            provider_status(def, merged_path, override_path, detected_shell)
                        }),
                    )
                })
                .collect::<Vec<_>>();

            handles
                .into_iter()
                .map(|(def, handle)| {
                    handle.join().unwrap_or_else(|_| {
                        errored_provider_status(def, "Provider detection panicked")
                    })
                })
                .collect::<Vec<_>>()
        });

        results.sort_by_key(|status| provider_order(status.id));
        results
    }

    pub fn definition(provider_id: &str) -> Option<&'static ProviderDef> {
        PROVIDERS
            .iter()
            .find(|provider| provider.id.as_str() == provider_id)
    }

    pub fn cli_command(provider_id: &str) -> Option<&'static str> {
        Self::definition(provider_id).map(|provider| provider.cli_command)
    }
    pub fn resolve_command_path(
        provider_id: &str,
        merged_path: &str,
        binary_override: Option<&str>,
    ) -> Option<String> {
        if let Some(override_path) = binary_override {
            let trimmed = override_path.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }

        let definition = Self::definition(provider_id)?;
        definition
            .detect_commands
            .iter()
            .find_map(|command| which::which_in(command, Some(merged_path), ".").ok())
            .map(|path| path.to_string_lossy().to_string())
            .or_else(|| Some(definition.cli_command.to_string()))
    }

    /// Build a provider-specific argument vector for session start/resume.
    pub fn build_start_args(
        provider_id: &str,
        resume_token: Option<&str>,
        session_app_id: Option<&str>,
    ) -> Vec<String> {
        let Some(definition) = Self::definition(provider_id) else {
            return Vec::new();
        };

        let mut args = Vec::new();

        match &definition.resume_mode {
            ResumeMode::None => {}
            ResumeMode::SessionId {
                session_flag,
                continue_flags,
            } => {
                if let Some(token) = resume_token {
                    // Resume existing session: e.g. `claude --resume <uuid>`
                    // The session ID is the VALUE of the resume flag, not --session-id
                    if let Some(flag) = continue_flags.first() {
                        args.push((*flag).to_string());
                        args.push(token.to_string());
                    }
                } else if let Some(app_id) = session_app_id {
                    // First start with a fresh ID: e.g. `claude --session-id <uuid>`
                    args.push((*session_flag).to_string());
                    args.push(app_id.to_string());
                }
            }
            ResumeMode::ThreadId { resume_command } => {
                if let Some(thread_id) = resume_token {
                    args.push((*resume_command).to_string());
                    args.push(thread_id.to_string());
                }
            }
            ResumeMode::ConversationId { flag } => {
                if let Some(conversation_id) = resume_token {
                    args.push((*flag).to_string());
                    args.push(conversation_id.to_string());
                }
            }
        }

        args.extend(definition.default_args.iter().map(|arg| (*arg).to_string()));

        args
    }
}

fn provider_status(
    definition: &ProviderDef,
    merged_path: &str,
    binary_override: Option<&str>,
    detected_shell: Option<&str>,
) -> ProviderStatus {
    if definition.id != ProviderId::Terminal {
        return detect_provider(definition, merged_path, binary_override);
    }

    let shell = detected_shell.unwrap_or("/bin/sh");
    let shell_name = std::path::Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("sh");
    ProviderStatus {
        id: definition.id,
        label: definition.label.to_string(),
        status: ProviderConnectionStatus::Connected,
        version: Some(shell_name.to_string()),
        resolved_path: Some(shell.to_string()),
        message: None,
        install_hint: String::new(),
    }
}

fn provider_order(id: ProviderId) -> usize {
    PROVIDERS
        .iter()
        .position(|provider| provider.id == id)
        .unwrap_or(usize::MAX)
}

fn errored_provider_status(definition: &ProviderDef, message: &str) -> ProviderStatus {
    ProviderStatus {
        id: definition.id,
        label: definition.label.to_string(),
        status: ProviderConnectionStatus::Error,
        version: None,
        resolved_path: None,
        message: Some(message.to_string()),
        install_hint: definition.install_hint.to_string(),
    }
}

fn detect_provider(
    definition: &ProviderDef,
    merged_path: &str,
    binary_override: Option<&str>,
) -> ProviderStatus {
    let resolved = if let Some(override_path) = binary_override {
        let trimmed = override_path.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else {
        definition
            .detect_commands
            .iter()
            .find_map(|command| which::which_in(command, Some(merged_path), ".").ok())
            .map(|path| path.to_string_lossy().to_string())
    };

    if let Some(ref path) = resolved {
        match std::process::Command::new(path)
            .args(definition.version_args)
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // NOTE: OMP prints `--version` on stderr and leaves stdout
                // empty. Other CLIs print to stdout; do not generalize the
                // fallback or stderr banners (deprecation, login warnings)
                // will surface as fake version strings.
                let version_source =
                    if matches!(definition.id, ProviderId::Omp) && stdout.trim().is_empty() {
                        String::from_utf8_lossy(&output.stderr).trim().to_string()
                    } else {
                        stdout.trim().to_string()
                    };
                let version = version_source.lines().next().unwrap_or("").to_string();
                info!("{} detected: {} ({})", definition.label, version, path);
                ProviderStatus {
                    id: definition.id,
                    label: definition.label.to_string(),
                    status: ProviderConnectionStatus::Connected,
                    version: Some(version),
                    resolved_path: resolved,
                    message: None,
                    install_hint: definition.install_hint.to_string(),
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                warn!(
                    "{} found but version check failed: {}",
                    definition.label, stderr
                );
                ProviderStatus {
                    id: definition.id,
                    label: definition.label.to_string(),
                    status: ProviderConnectionStatus::Error,
                    version: None,
                    resolved_path: resolved,
                    message: Some(format!("Version check failed: {}", stderr.trim())),
                    install_hint: definition.install_hint.to_string(),
                }
            }
            Err(error) => {
                warn!(
                    "{} found but could not execute: {}",
                    definition.label, error
                );
                ProviderStatus {
                    id: definition.id,
                    label: definition.label.to_string(),
                    status: ProviderConnectionStatus::Error,
                    version: None,
                    resolved_path: resolved,
                    message: Some(format!("Execution failed: {}", error)),
                    install_hint: definition.install_hint.to_string(),
                }
            }
        }
    } else {
        ProviderStatus {
            id: definition.id,
            label: definition.label.to_string(),
            status: ProviderConnectionStatus::Missing,
            version: None,
            resolved_path: None,
            message: None,
            install_hint: definition.install_hint.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Encoding observed in real ~/.claude/projects/* dirs.
    #[test]
    fn claude_project_dir_simple_path() {
        assert_eq!(
            claude_project_dir_name("/home/toph/Development/ADE"),
            "-home-toph-Development-ADE"
        );
    }

    #[test]
    fn claude_project_dir_dot_in_segment() {
        // `/home/toph/.catnip/workspace/KwahsCore/fluffy` was observed
        // on disk as `-home-toph--catnip-workspace-KwahsCore-fluffy`.
        assert_eq!(
            claude_project_dir_name("/home/toph/.catnip/workspace/KwahsCore/fluffy"),
            "-home-toph--catnip-workspace-KwahsCore-fluffy"
        );
    }

    #[test]
    fn claude_project_dir_hyphens_pass_through() {
        // `/home/toph/Development/nerf-this` was observed on disk as
        // `-home-toph-Development-nerf-this` (existing hyphens stay).
        assert_eq!(
            claude_project_dir_name("/home/toph/Development/nerf-this"),
            "-home-toph-Development-nerf-this"
        );
    }

    #[test]
    fn claude_transcript_path_shape() {
        std::env::set_var("HOME", "/tmp/fakehome");
        let path = claude_transcript_path("/repo/x", "abc-123").unwrap();
        assert_eq!(
            path,
            PathBuf::from("/tmp/fakehome/.claude/projects/-repo-x/abc-123.jsonl")
        );
    }
}
