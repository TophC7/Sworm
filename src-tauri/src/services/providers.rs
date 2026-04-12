use crate::models::provider::{
    PromptMode, ProviderConnectionStatus, ProviderId, ProviderStatus, ResumeMode, SessionIdMode,
};
use std::collections::HashMap;
use tracing::{info, warn};

/// Static provider definitions for Phase 1.
#[allow(dead_code)]
pub struct ProviderDef {
    pub id: ProviderId,
    pub label: &'static str,
    pub cli_command: &'static str,
    pub detect_commands: &'static [&'static str],
    pub version_args: &'static [&'static str],
    pub install_hint: &'static str,
    pub docs_url: &'static str,
    pub auto_approve_flag: Option<&'static str>,
    pub prompt_mode: PromptMode,
    pub resume_mode: ResumeMode,
    pub session_id_mode: SessionIdMode,
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
        docs_url: "https://docs.anthropic.com/en/docs/claude-code",
        auto_approve_flag: Some("--dangerously-skip-permissions"),
        prompt_mode: PromptMode::ArgvTail,
        resume_mode: ResumeMode::SessionId {
            session_flag: "--session-id",
            continue_flags: &["--resume"],
        },
        session_id_mode: SessionIdMode::Deterministic {
            flag: "--session-id",
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
        docs_url: "https://github.com/openai/codex",
        auto_approve_flag: Some("--full-auto"),
        prompt_mode: PromptMode::ArgvTail,
        resume_mode: ResumeMode::ThreadId {
            resume_command: "resume",
        },
        session_id_mode: SessionIdMode::None,
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Copilot,
        label: "GitHub Copilot",
        cli_command: "copilot",
        detect_commands: &["copilot"],
        version_args: &["--version"],
        install_hint: "",
        docs_url: "https://docs.github.com/en/copilot",
        auto_approve_flag: Some("--allow-all-tools"),
        prompt_mode: PromptMode::FlagThenValue { flag: "-i" },
        resume_mode: ResumeMode::SessionId {
            session_flag: "--resume",
            continue_flags: &["--resume"],
        },
        session_id_mode: SessionIdMode::Deterministic { flag: "--resume" },
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Crush,
        label: "Crush",
        cli_command: "crush",
        detect_commands: &["crush"],
        version_args: &["--version"],
        install_hint: "",
        docs_url: "https://github.com/charmbracelet/crush",
        auto_approve_flag: Some("--yolo"),
        prompt_mode: PromptMode::KeystrokeInjection,
        resume_mode: ResumeMode::None,
        session_id_mode: SessionIdMode::None,
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Gemini,
        label: "Gemini CLI",
        cli_command: "gemini",
        detect_commands: &["gemini"],
        version_args: &["--version"],
        install_hint: "",
        docs_url: "https://github.com/google-gemini/gemini-cli",
        auto_approve_flag: Some("--yolo"),
        prompt_mode: PromptMode::FlagThenValue { flag: "-i" },
        resume_mode: ResumeMode::GenericFlag {
            flags: &["--resume", "latest"],
        },
        session_id_mode: SessionIdMode::None,
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Fresh,
        label: "Fresh",
        cli_command: "fresh",
        detect_commands: &["fresh"],
        version_args: &["--version"],
        install_hint: "Fresh is not installed",
        docs_url: "",
        auto_approve_flag: None,
        prompt_mode: PromptMode::KeystrokeInjection,
        resume_mode: ResumeMode::None,
        session_id_mode: SessionIdMode::None,
        default_args: &[],
    },
    ProviderDef {
        id: ProviderId::Terminal,
        label: "Terminal",
        cli_command: "sh",
        detect_commands: &[], // detected via $SHELL, not PATH lookup
        version_args: &["--version"],
        install_hint: "",
        docs_url: "",
        auto_approve_flag: None,
        prompt_mode: PromptMode::KeystrokeInjection,
        resume_mode: ResumeMode::None,
        session_id_mode: SessionIdMode::None,
        default_args: &[],
    },
];

/// Provider service: detection, registry, status caching.
pub struct ProviderService {
    cache: HashMap<String, ProviderStatus>,
}

impl ProviderService {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    pub fn detect_all(
        &mut self,
        merged_path: &str,
        binary_overrides: &HashMap<String, String>,
        detected_shell: Option<&str>,
    ) -> Vec<ProviderStatus> {
        let mut results = Vec::new();

        for def in PROVIDERS {
            let override_path = binary_overrides
                .get(&def.id.to_string())
                .map(String::as_str);

            let status = if def.id == ProviderId::Terminal {
                // Terminal is always available — it uses the user's login shell
                let shell = detected_shell.unwrap_or("/bin/sh");
                let shell_name = std::path::Path::new(shell)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("sh");
                ProviderStatus {
                    id: def.id,
                    label: def.label.to_string(),
                    status: ProviderConnectionStatus::Connected,
                    version: Some(shell_name.to_string()),
                    resolved_path: Some(shell.to_string()),
                    message: None,
                    install_hint: String::new(),
                }
            } else {
                detect_provider(def, merged_path, override_path)
            };

            self.cache.insert(def.id.to_string(), status.clone());
            results.push(status);
        }

        results
    }

    pub fn list(
        &mut self,
        merged_path: &str,
        binary_overrides: &HashMap<String, String>,
        detected_shell: Option<&str>,
    ) -> Vec<ProviderStatus> {
        self.detect_all(merged_path, binary_overrides, detected_shell)
    }

    pub fn definition(provider_id: &str) -> Option<&'static ProviderDef> {
        PROVIDERS
            .iter()
            .find(|provider| provider.id.to_string() == provider_id)
    }

    pub fn exists(provider_id: &str) -> bool {
        Self::definition(provider_id).is_some()
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
        auto_approve: bool,
        resume_token: Option<&str>,
        session_app_id: Option<&str>,
        initial_prompt: Option<&str>,
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
                    // First start with deterministic ID: e.g. `claude --session-id <uuid>`
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
            ResumeMode::GenericFlag { flags } => {
                // resume_token being Some signals "this is a restart, use resume flags"
                if resume_token.is_some() {
                    for flag in *flags {
                        args.push((*flag).to_string());
                    }
                }
            }
        }

        if auto_approve {
            if let Some(flag) = definition.auto_approve_flag {
                args.push(flag.to_string());
            }
        }

        args.extend(definition.default_args.iter().map(|arg| (*arg).to_string()));

        match &definition.prompt_mode {
            PromptMode::ArgvTail => {
                if let Some(prompt) = initial_prompt {
                    args.push(prompt.to_string());
                }
            }
            PromptMode::FlagThenValue { flag } => {
                if let Some(prompt) = initial_prompt {
                    args.push((*flag).to_string());
                    args.push(prompt.to_string());
                }
            }
            PromptMode::KeystrokeInjection => {}
        }

        args
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
                let version = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string();
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
