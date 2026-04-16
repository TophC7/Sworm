use crate::models::nix_env::{NixEnvRecord, NixEnvStatus};
use crate::services::env::merge_paths;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use tracing::info;

/// Nix files checked in priority order.
const NIX_FILES: &[&str] = &["flake.nix", "shell.nix", "default.nix"];

/// Environment variables that the host always owns — Nix cannot override these.
/// These provide system integration (display, auth, API keys) that must come
/// from the running desktop session, not from a Nix evaluation.
///
/// Keep in sync with ENV_ALLOWLIST in services/env.rs — any var that should
/// survive a Nix overlay must appear here.
const HOST_AUTHORITATIVE: &[&str] = &[
    "TERM",
    "COLORTERM",
    "HOME",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "GDK_BACKEND",
    "XDG_RUNTIME_DIR",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_TYPE",
    "DBUS_SESSION_BUS_ADDRESS",
    "SSH_AUTH_SOCK",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
];

/// Errors that can occur during Nix evaluation.
#[derive(Debug)]
pub enum NixEvalError {
    Timeout,
    NixNotFound,
    CommandFailed {
        stderr: String,
        exit_code: Option<i32>,
    },
    ParseError(String),
}

impl std::error::Error for NixEvalError {}

impl std::fmt::Display for NixEvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "Nix evaluation timed out (120s)"),
            Self::NixNotFound => write!(f, "nix is not installed or not on PATH"),
            Self::CommandFailed { stderr, exit_code } => {
                write!(
                    f,
                    "Nix exited with code {}: {}",
                    exit_code
                        .map(|c| c.to_string())
                        .unwrap_or_else(|| "signal".to_string()),
                    stderr
                )
            }
            Self::ParseError(msg) => write!(f, "Failed to parse Nix env output: {}", msg),
        }
    }
}

/// Diagnostic from `nix-instantiate --parse` stderr.
#[derive(Debug, serde::Serialize)]
pub struct NixDiagnostic {
    pub message: String,
    pub line: u32,
    pub column: u32,
}

/// Stateless Nix environment service. All persistent state lives in the database.
pub struct NixService;

impl NixService {
    /// Scan a project directory for known Nix files.
    pub fn detect(project_path: &str) -> Vec<String> {
        let root = Path::new(project_path);
        NIX_FILES
            .iter()
            .filter(|f| root.join(f).exists())
            .map(|f| f.to_string())
            .collect()
    }

    /// Load the Nix env record for a project from the database.
    pub fn get(conn: &Connection, project_id: &str) -> Result<Option<NixEnvRecord>, String> {
        conn.query_row(
            "SELECT project_id, nix_file, status, env_json, error_message, \
             evaluated_at, created_at, updated_at \
             FROM project_nix_envs WHERE project_id = ?1",
            [project_id],
            |row| {
                let status_str: String = row.get(2)?;
                Ok(NixEnvRecord {
                    project_id: row.get(0)?,
                    nix_file: row.get(1)?,
                    status: NixEnvStatus::from_db_str(&status_str),
                    env_json: row.get(3)?,
                    error_message: row.get(4)?,
                    evaluated_at: row.get(5)?,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    /// Select a Nix file for a project. Creates or updates the DB row
    /// with status=pending.
    pub fn select(
        conn: &Connection,
        project_id: &str,
        nix_file: &str,
    ) -> Result<NixEnvRecord, String> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO project_nix_envs (project_id, nix_file, status, created_at, updated_at) \
             VALUES (?1, ?2, 'pending', ?3, ?4) \
             ON CONFLICT(project_id) DO UPDATE SET \
             nix_file = excluded.nix_file, \
             status = 'pending', \
             env_json = NULL, \
             error_message = NULL, \
             evaluated_at = NULL, \
             updated_at = excluded.updated_at",
            rusqlite::params![project_id, nix_file, now, now],
        )
        .map_err(|e| e.to_string())?;

        Self::get(conn, project_id)?.ok_or_else(|| "Failed to read back Nix env record".to_string())
    }

    /// Remove the Nix env association for a project.
    pub fn remove(conn: &Connection, project_id: &str) -> Result<(), String> {
        conn.execute(
            "DELETE FROM project_nix_envs WHERE project_id = ?1",
            [project_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Update the DB row to reflect evaluation status.
    pub fn set_status(
        conn: &Connection,
        project_id: &str,
        status: NixEnvStatus,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE project_nix_envs SET status = ?1, updated_at = ?2 WHERE project_id = ?3",
            rusqlite::params![status.as_str(), now, project_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Save a successful evaluation result to the database.
    pub fn save_success(
        conn: &Connection,
        project_id: &str,
        env_vars: &HashMap<String, String>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let json = serde_json::to_string(env_vars).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE project_nix_envs SET status = 'ready', env_json = ?1, \
             error_message = NULL, evaluated_at = ?2, updated_at = ?3 \
             WHERE project_id = ?4",
            rusqlite::params![json, now, now, project_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Save a failed evaluation result to the database.
    pub fn save_error(
        conn: &Connection,
        project_id: &str,
        error: &NixEvalError,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let status = match error {
            NixEvalError::Timeout => "timeout",
            _ => "error",
        };
        conn.execute(
            "UPDATE project_nix_envs SET status = ?1, env_json = NULL, \
             error_message = ?2, evaluated_at = ?3, updated_at = ?4 \
             WHERE project_id = ?5",
            rusqlite::params![status, error.to_string(), now, now, project_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Evaluate a Nix expression and capture the resulting environment.
    ///
    /// This is a blocking operation that can take 30+ seconds. Call from
    /// a blocking thread context.
    pub fn evaluate(
        project_path: &str,
        nix_file: &str,
        timeout_secs: u64,
    ) -> Result<HashMap<String, String>, NixEvalError> {
        let is_flake = nix_file == "flake.nix";
        let required_bin = if is_flake { "nix" } else { "nix-shell" };
        if which::which(required_bin).is_err() {
            return Err(NixEvalError::NixNotFound);
        }
        let timeout = std::time::Duration::from_secs(timeout_secs);

        info!(
            "Evaluating Nix env: {} in {} (timeout={}s)",
            nix_file, project_path, timeout_secs
        );

        let mut cmd = if is_flake {
            let mut c = std::process::Command::new("nix");
            c.args([
                "develop",
                project_path,
                "--no-write-lock-file",
                "--command",
                "env",
                "-0",
            ]);
            c
        } else {
            let file_path = Path::new(project_path).join(nix_file);
            let mut c = std::process::Command::new("nix-shell");
            c.args([file_path.to_str().unwrap_or(nix_file), "--run", "env -0"]);
            c
        };

        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| NixEvalError::CommandFailed {
            stderr: e.to_string(),
            exit_code: None,
        })?;

        // Wait with timeout
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let output =
                        child
                            .wait_with_output()
                            .map_err(|e| NixEvalError::CommandFailed {
                                stderr: e.to_string(),
                                exit_code: None,
                            })?;

                    if !status.success() {
                        return Err(NixEvalError::CommandFailed {
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                            exit_code: status.code(),
                        });
                    }

                    return parse_env_output(&output.stdout);
                }
                Ok(None) => {
                    if start.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(NixEvalError::Timeout);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(NixEvalError::CommandFailed {
                        stderr: e.to_string(),
                        exit_code: None,
                    });
                }
            }
        }
    }

    /// Merge a Nix-captured environment with the host's base environment.
    ///
    /// Strategy:
    /// - Host-authoritative vars (display, auth, API keys) are never overridden
    /// - PATH is merged: Nix entries prepended, host entries appended, deduplicated
    /// - Everything else from Nix is added or overwrites the host value
    pub fn merge_env(
        host_env: &HashMap<String, String>,
        nix_env: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut merged = host_env.clone();

        for (key, value) in nix_env {
            // Never override host-authoritative vars
            if HOST_AUTHORITATIVE.contains(&key.as_str()) {
                continue;
            }

            if key == "PATH" {
                // Prepend Nix PATH entries before host PATH
                let host_path = host_env.get("PATH").map(|s| s.as_str()).unwrap_or("");
                merged.insert("PATH".to_string(), merge_paths(value, host_path));
            } else {
                // Nix value wins for everything else
                merged.insert(key.clone(), value.clone());
            }
        }

        merged
    }

    /// Extract a merged PATH from a Nix env + host PATH, for provider detection.
    pub fn merged_path(host_path: &str, nix_env: &HashMap<String, String>) -> String {
        match nix_env.get("PATH") {
            Some(nix_path) => merge_paths(nix_path, host_path),
            None => host_path.to_string(),
        }
    }

    /// Load the cached Nix env vars from the DB for a project.
    /// Returns None if no Nix env is configured or not yet evaluated.
    pub fn load_env_vars(
        conn: &Connection,
        project_id: &str,
    ) -> Result<Option<HashMap<String, String>>, String> {
        let record = Self::get(conn, project_id)?;
        match record {
            Some(rec) if rec.status == NixEnvStatus::Ready => {
                if let Some(ref json) = rec.env_json {
                    let vars: HashMap<String, String> =
                        serde_json::from_str(json).map_err(|e| e.to_string())?;
                    Ok(Some(vars))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// Format Nix source via `nixfmt` (stdin/stdout pipe).
    pub fn format_nix(content: &str) -> Result<String, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut child = Command::new("nixfmt")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("nixfmt not found: {e}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(content.as_bytes())
                .map_err(|e| format!("failed to write to nixfmt stdin: {e}"))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|e| format!("nixfmt failed: {e}"))?;

        if output.status.success() {
            String::from_utf8(output.stdout)
                .map_err(|e| format!("nixfmt output not valid UTF-8: {e}"))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("nixfmt error: {stderr}"))
        }
    }

    /// Parse-check a Nix file and return diagnostics from stderr.
    ///
    /// nix-instantiate errors look like:
    ///   error: <message>, at /path/to/file.nix:<line>:<col>
    pub fn lint_nix(file_path: &str) -> Result<Vec<NixDiagnostic>, String> {
        let output = std::process::Command::new("nix-instantiate")
            .args(["--parse", file_path])
            .output()
            .map_err(|e| format!("nix-instantiate not found: {e}"))?;

        if output.status.success() {
            return Ok(vec![]);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let diagnostics = stderr
            .lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("error: ")?;
                // Split on ", at " to separate message from location
                let (message, location) = rest.rsplit_once(", at ")?;
                // Location is "path:line:col" — extract after last ':'s
                let (path_and_line, col_str) = location.rsplit_once(':')?;
                let (_, line_str) = path_and_line.rsplit_once(':')?;
                Some(NixDiagnostic {
                    message: message.to_string(),
                    line: line_str.parse().ok()?,
                    column: col_str.parse().ok()?,
                })
            })
            .collect();

        Ok(diagnostics)
    }
}

/// Parse null-delimited `env -0` output into a HashMap.
fn parse_env_output(output: &[u8]) -> Result<HashMap<String, String>, NixEvalError> {
    let raw = String::from_utf8_lossy(output);
    let mut env = HashMap::new();

    for entry in raw.split('\0') {
        if entry.is_empty() {
            continue;
        }
        if let Some(eq_pos) = entry.find('=') {
            let key = &entry[..eq_pos];
            let value = &entry[eq_pos + 1..];
            // Skip entries with empty keys or internal Nix vars that are noise
            if !key.is_empty() && !key.starts_with("BASH_FUNC_") {
                env.insert(key.to_string(), value.to_string());
            }
        }
    }

    if env.is_empty() {
        return Err(NixEvalError::ParseError(
            "No environment variables captured from Nix output".to_string(),
        ));
    }

    info!("Parsed {} env vars from Nix evaluation", env.len());
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_env_output_basic() {
        let output = b"FOO=bar\0BAZ=qux\0";
        let env = parse_env_output(output).unwrap();
        assert_eq!(env.get("FOO").unwrap(), "bar");
        assert_eq!(env.get("BAZ").unwrap(), "qux");
    }

    #[test]
    fn test_parse_env_output_multiline_value() {
        let output = b"MULTI=line1\nline2\0SINGLE=val\0";
        let env = parse_env_output(output).unwrap();
        assert_eq!(env.get("MULTI").unwrap(), "line1\nline2");
        assert_eq!(env.get("SINGLE").unwrap(), "val");
    }

    #[test]
    fn test_parse_env_output_empty() {
        let output = b"";
        assert!(parse_env_output(output).is_err());
    }

    #[test]
    fn test_parse_env_output_skips_bash_funcs() {
        let output = b"PATH=/usr/bin\0BASH_FUNC_foo%%=() { echo; }\0";
        let env = parse_env_output(output).unwrap();
        assert!(env.contains_key("PATH"));
        assert!(!env.keys().any(|k| k.starts_with("BASH_FUNC_")));
    }

    #[test]
    fn test_merge_env_host_authoritative_preserved() {
        let mut host = HashMap::new();
        host.insert("HOME".to_string(), "/home/user".to_string());
        host.insert("DISPLAY".to_string(), ":0".to_string());
        host.insert("CC".to_string(), "gcc".to_string());

        let mut nix = HashMap::new();
        nix.insert("HOME".to_string(), "/homeless-shelter".to_string());
        nix.insert("DISPLAY".to_string(), "nix-display".to_string());
        nix.insert("CC".to_string(), "/nix/store/.../cc".to_string());
        nix.insert("NEW_VAR".to_string(), "from-nix".to_string());

        let merged = NixService::merge_env(&host, &nix);
        assert_eq!(merged.get("HOME").unwrap(), "/home/user");
        assert_eq!(merged.get("DISPLAY").unwrap(), ":0");
        assert_eq!(merged.get("CC").unwrap(), "/nix/store/.../cc");
        assert_eq!(merged.get("NEW_VAR").unwrap(), "from-nix");
    }

    #[test]
    fn test_merge_env_path_prepended() {
        let mut host = HashMap::new();
        host.insert("PATH".to_string(), "/usr/bin:/usr/local/bin".to_string());

        let mut nix = HashMap::new();
        nix.insert(
            "PATH".to_string(),
            "/nix/store/a:/nix/store/b:/usr/bin".to_string(),
        );

        let merged = NixService::merge_env(&host, &nix);
        let path = merged.get("PATH").unwrap();
        assert!(path.starts_with("/nix/store/a"));
        // /usr/bin should appear only once (deduplicated)
        assert_eq!(path.matches("/usr/bin").count(), 1);
    }

    #[test]
    fn test_merged_path_with_nix() {
        let mut nix = HashMap::new();
        nix.insert("PATH".to_string(), "/nix/store/bin:/usr/bin".to_string());
        let result = NixService::merged_path("/usr/bin:/usr/local/bin", &nix);
        assert!(result.starts_with("/nix/store/bin"));
        assert!(result.contains("/usr/local/bin"));
    }

    #[test]
    fn test_merged_path_without_nix() {
        let nix = HashMap::new();
        let result = NixService::merged_path("/usr/bin", &nix);
        assert_eq!(result, "/usr/bin");
    }
}
