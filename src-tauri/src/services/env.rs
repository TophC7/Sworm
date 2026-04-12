use serde::Serialize;
use std::collections::HashMap;
use tracing::{info, warn};

/// Allowlisted environment variables that are safe to pass to child processes.
const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "COLORTERM",
    "SSH_AUTH_SOCK",
    "GDK_BACKEND",
    "XDG_RUNTIME_DIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
];

/// Environment bootstrap service.
///
/// Captures a sanitized base environment at startup and probes the
/// user's login shell for PATH so provider CLIs installed under
/// user-managed toolchains are discoverable even when the app is
/// launched from a desktop entry.
pub struct EnvironmentService {
    /// The user's login shell (from $SHELL)
    pub detected_shell: String,
    /// PATH as inherited by the Tauri process
    pub base_path: String,
    /// PATH obtained by probing the login shell
    pub shell_path: Option<String>,
    /// Merged PATH: shell_path preferred, fallback to base_path
    pub merged_path: String,
    /// Whether the shell probe succeeded
    pub probe_succeeded: bool,
    /// Merged environment for child processes
    pub child_env: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvProbeResult {
    pub detected_shell: String,
    pub base_path: String,
    pub shell_path: Option<String>,
    pub merged_path: String,
    pub probe_succeeded: bool,
    pub gdk_backend: Option<String>,
    pub webkit_disable_dmabuf_renderer: Option<String>,
    pub webkit_disable_compositing_mode: Option<String>,
}

impl EnvironmentService {
    /// Bootstrap the environment service.
    ///
    /// 1. Capture the base environment
    /// 2. Detect the user shell
    /// 3. Probe the login shell for PATH
    /// 4. Build a merged child environment
    pub fn new() -> Self {
        let detected_shell = detect_login_shell();
        let base_path = std::env::var("PATH").unwrap_or_default();

        info!("Environment bootstrap: shell={}, base PATH length={}", detected_shell, base_path.len());

        // Probe the login shell for its PATH
        let (shell_path, probe_succeeded) = probe_shell_path(&detected_shell);

        let merged_path = if let Some(ref sp) = shell_path {
            merge_paths(sp, &base_path)
        } else {
            base_path.clone()
        };

        info!(
            "Environment bootstrap: probe_succeeded={}, merged PATH length={}",
            probe_succeeded,
            merged_path.len()
        );

        // Build the allowlisted child environment
        let mut child_env = HashMap::new();
        for &key in ENV_ALLOWLIST {
            if key == "PATH" {
                child_env.insert("PATH".to_string(), merged_path.clone());
            } else if let Ok(val) = std::env::var(key) {
                child_env.insert(key.to_string(), val);
            }
        }

        // Ensure TERM is set
        child_env
            .entry("TERM".to_string())
            .or_insert_with(|| "xterm-256color".to_string());

        Self {
            detected_shell,
            base_path,
            shell_path,
            merged_path,
            probe_succeeded,
            child_env,
        }
    }

    /// Return a diagnostic probe result for the frontend.
    pub fn probe_result(&self) -> EnvProbeResult {
        EnvProbeResult {
            detected_shell: self.detected_shell.clone(),
            base_path: self.base_path.clone(),
            shell_path: self.shell_path.clone(),
            merged_path: self.merged_path.clone(),
            probe_succeeded: self.probe_succeeded,
            gdk_backend: std::env::var("GDK_BACKEND").ok(),
            webkit_disable_dmabuf_renderer: std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").ok(),
            webkit_disable_compositing_mode: std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").ok(),
        }
    }
}

/// Detect the user's login shell.
///
/// `$SHELL` can be overridden by tooling (e.g. `nix develop` sets it to
/// a Nix-store bash). The authoritative source is `/etc/passwd`, so we
/// check that first via `getent passwd $USER` and fall back to `$SHELL`.
fn detect_login_shell() -> String {
    // Check /etc/passwd — this is the user's configured login shell
    if let Ok(user) = std::env::var("USER") {
        if let Ok(output) = std::process::Command::new("getent")
            .args(["passwd", &user])
            .output()
        {
            if output.status.success() {
                let line = String::from_utf8_lossy(&output.stdout);
                if let Some(shell) = line.trim().rsplit(':').next() {
                    if !shell.is_empty()
                        && shell != "/bin/false"
                        && shell != "/usr/sbin/nologin"
                    {
                        info!("Login shell from /etc/passwd: {}", shell);
                        return shell.to_string();
                    }
                }
            }
        }
    }

    // Fall back to $SHELL
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    info!("Login shell from $SHELL: {}", shell);
    shell
}

/// Probe the user's login shell for its PATH.
///
/// Runs: `$SHELL -lc 'printf "%s" "$PATH"'`
fn probe_shell_path(shell: &str) -> (Option<String>, bool) {
    let result = std::process::Command::new(shell)
        .args(["-lc", r#"printf "%s" "$PATH""#])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            let path = String::from_utf8_lossy(&output.stdout).to_string();
            if path.is_empty() {
                warn!("Shell probe returned empty PATH");
                (None, false)
            } else {
                info!("Shell probe succeeded, PATH has {} entries", path.split(':').count());
                (Some(path), true)
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Shell probe exited with {}: {}", output.status, stderr);
            (None, false)
        }
        Err(e) => {
            warn!("Shell probe failed to execute: {}", e);
            (None, false)
        }
    }
}

/// Merge two PATH strings, preferring entries from `primary` but
/// appending unique entries from `secondary`.
pub(crate) fn merge_paths(primary: &str, secondary: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut parts = Vec::new();

    for entry in primary.split(':') {
        if !entry.is_empty() && seen.insert(entry.to_string()) {
            parts.push(entry.to_string());
        }
    }
    for entry in secondary.split(':') {
        if !entry.is_empty() && seen.insert(entry.to_string()) {
            parts.push(entry.to_string());
        }
    }

    parts.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_paths_deduplicates() {
        let result = merge_paths("/usr/bin:/usr/local/bin", "/usr/bin:/opt/bin");
        assert_eq!(result, "/usr/bin:/usr/local/bin:/opt/bin");
    }

    #[test]
    fn test_merge_paths_empty() {
        let result = merge_paths("", "/usr/bin");
        assert_eq!(result, "/usr/bin");
    }

    #[test]
    fn test_merge_paths_primary_only() {
        let result = merge_paths("/a:/b", "");
        assert_eq!(result, "/a:/b");
    }
}
