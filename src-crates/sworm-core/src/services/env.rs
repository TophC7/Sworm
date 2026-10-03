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
    /// Merged environment for child processes
    pub child_env: HashMap<String, String>,
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

        info!(
            "Environment bootstrap: shell={}, base PATH length={}",
            detected_shell,
            base_path.len()
        );

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
            if key != "PATH" {
                if let Ok(val) = std::env::var(key) {
                    child_env.insert(key.to_string(), val);
                }
            }
        }
        child_env.insert("PATH".to_string(), merged_path);

        // Ensure TERM is set
        child_env
            .entry("TERM".to_string())
            .or_insert_with(|| "xterm-256color".to_string());

        // Guarantee 24-bit color support to child CLIs. Without this,
        // some agent CLIs render monochrome because their capability
        // probe checks COLORTERM before trusting the bare
        // TERM value. We only set the default when the parent didn't
        // provide one, so launches from a colour-aware terminal (e.g.
        // Ghostty setting `truecolor`) win over the default.
        child_env
            .entry("COLORTERM".to_string())
            .or_insert_with(|| "truecolor".to_string());

        Self {
            detected_shell,
            child_env,
        }
    }

    /// Login-shell PATH preferred, falling back to the host process PATH.
    pub fn path(&self) -> &str {
        &self.child_env["PATH"]
    }

    /// Overlay Nix tools while preserving host session integration and credentials.
    pub fn with_nix(&self, nix: Option<&HashMap<String, String>>) -> HashMap<String, String> {
        let mut merged = self.child_env.clone();
        let Some(nix) = nix else {
            return merged;
        };

        for (key, value) in nix {
            if key == "PATH" {
                merged.insert(key.clone(), merge_paths(value, self.path()));
            } else if !ENV_ALLOWLIST.contains(&key.as_str()) {
                merged.insert(key.clone(), value.clone());
            }
        }

        // NixOS shell init re-sources set-environment, replacing PATH, unless
        // this is set. Session-launched hosts pass it through `nix develop`; a
        // systemd daemon has none, so its fish children would drop the devshell.
        merged
            .entry("__NIXOS_SET_ENVIRONMENT_DONE".to_string())
            .or_insert_with(|| "1".to_string());
        merged
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
                    if !shell.is_empty() && shell != "/bin/false" && shell != "/usr/sbin/nologin" {
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
                info!(
                    "Shell probe succeeded, PATH has {} entries",
                    path.split(':').count()
                );
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
fn merge_paths(primary: &str, secondary: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut parts = Vec::new();

    for entry in primary.split(':').chain(secondary.split(':')) {
        if !entry.is_empty() && seen.insert(entry) {
            parts.push(entry);
        }
    }

    parts.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_env_host_authoritative_preserved() {
        let mut child_env = HashMap::new();
        let mut nix = HashMap::new();
        for &key in ENV_ALLOWLIST {
            if key != "PATH" {
                child_env.insert(key.to_string(), format!("host-{key}"));
                nix.insert(key.to_string(), format!("nix-{key}"));
            }
        }
        child_env.insert("PATH".to_string(), "/usr/bin".to_string());
        child_env.insert("CC".to_string(), "gcc".to_string());
        nix.insert("CC".to_string(), "/nix/store/cc".to_string());
        nix.insert("NEW_VAR".to_string(), "from-nix".to_string());
        let env = EnvironmentService {
            detected_shell: "/bin/sh".to_string(),
            child_env,
        };

        let merged = env.with_nix(Some(&nix));
        for &key in ENV_ALLOWLIST {
            if key != "PATH" {
                assert_eq!(merged.get(key), env.child_env.get(key), "{key}");
            }
        }
        assert_eq!(merged["PATH"], "/usr/bin");
        assert_eq!(merged["CC"], "/nix/store/cc");
        assert_eq!(merged["NEW_VAR"], "from-nix");
        assert_eq!(merged["__NIXOS_SET_ENVIRONMENT_DONE"], "1");
    }

    #[test]
    fn test_merge_env_path_prepended() {
        let env = EnvironmentService {
            detected_shell: "/bin/sh".to_string(),
            child_env: HashMap::from([("PATH".to_string(), "/usr/bin:/usr/local/bin".to_string())]),
        };
        let nix = HashMap::from([(
            "PATH".to_string(),
            "/nix/store/a:/nix/store/b:/usr/bin".to_string(),
        )]);

        let merged = env.with_nix(Some(&nix));
        assert_eq!(
            merged["PATH"],
            "/nix/store/a:/nix/store/b:/usr/bin:/usr/local/bin"
        );
        assert_eq!(env.path(), "/usr/bin:/usr/local/bin");
    }

    #[test]
    fn nix_cannot_supply_missing_host_authoritative_vars() {
        let env = EnvironmentService {
            detected_shell: "/bin/sh".to_string(),
            child_env: HashMap::from([("PATH".to_string(), "/usr/bin".to_string())]),
        };
        let nix = ENV_ALLOWLIST
            .iter()
            .filter(|&&key| key != "PATH")
            .map(|&key| (key.to_string(), "from-nix".to_string()))
            .collect();
        let merged = env.with_nix(Some(&nix));
        for &key in ENV_ALLOWLIST {
            if key != "PATH" {
                assert!(!merged.contains_key(key), "{key}");
            }
        }
        assert_eq!(env.with_nix(None), env.child_env);
    }

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
