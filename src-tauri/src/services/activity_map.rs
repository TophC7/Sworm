use crate::models::activity_map::{DiscoveredProject, DiscoveredProviderActivity};
use chrono::{Local, NaiveDateTime, TimeZone};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Maximum number of discovered projects to return.
const MAX_RESULTS: usize = 50;

/// Number of trailing days to track in daily_counts.
const WINDOW_DAYS: usize = 7;

const PROVIDER_CLAUDE_CODE: &str = "claude_code";
const PROVIDER_CODEX: &str = "codex";
const PROVIDER_COPILOT: &str = "copilot";
const PROVIDER_GEMINI: &str = "gemini";
const PROVIDER_CRUSH: &str = "crush";
const PROVIDER_OPENCODE: &str = "opencode";

pub struct ActivityMapService;

impl ActivityMapService {
    /// Scan all external agent CLIs for project history, merge results, and
    /// cross-reference against Sworm's known projects.
    ///
    /// `sworm_projects` is a list of `(path, project_id)` pairs.
    pub fn scan(sworm_projects: &[(String, String)]) -> Vec<DiscoveredProject> {
        let sworm_map: HashMap<&str, &str> = sworm_projects
            .iter()
            .map(|(path, id)| (path.as_str(), id.as_str()))
            .collect();

        let (day_starts, _) = day_boundaries();

        // Collect (path, activity) pairs from each provider
        let mut by_path: HashMap<String, Vec<DiscoveredProviderActivity>> = HashMap::new();

        let scanners: Vec<fn(&[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)>> = vec![
            scan_claude_code,
            scan_codex,
            scan_copilot,
            scan_gemini,
            scan_crush,
            scan_opencode,
        ];

        for scanner in scanners {
            for (path, activity) in scanner(&day_starts) {
                by_path.entry(path).or_default().push(activity);
            }
        }

        // Build DiscoveredProject for each unique path
        let mut results: Vec<DiscoveredProject> = by_path
            .into_iter()
            .map(|(path, providers)| {
                let last_active = providers
                    .iter()
                    .map(|p| p.last_active.as_str())
                    .max()
                    .unwrap_or("")
                    .to_string();

                let name = Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.clone());

                let path_exists = Path::new(&path).is_dir();

                let (is_sworm, sworm_id) = match sworm_map.get(path.as_str()) {
                    Some(id) => (true, Some(id.to_string())),
                    None => (false, None),
                };

                DiscoveredProject {
                    path,
                    name,
                    path_exists,
                    is_sworm_project: is_sworm,
                    sworm_project_id: sworm_id,
                    last_active,
                    providers,
                }
            })
            .collect();

        results.sort_by(|a, b| b.last_active.cmp(&a.last_active));
        results.truncate(MAX_RESULTS);
        results
    }
}

// -- Timestamp helpers --

/// Compute the start-of-day boundary for each of the trailing 7 days.
/// Returns (day_starts, window_start) where day_starts[0] is 6 days ago
/// and day_starts[6] is today.
fn day_boundaries() -> ([i64; WINDOW_DAYS], i64) {
    let today = Local::now().date_naive();
    let mut starts = [0i64; WINDOW_DAYS];
    for i in 0..WINDOW_DAYS {
        let date = today - chrono::Duration::days((WINDOW_DAYS - 1 - i) as i64);
        if let Some(dt) = date.and_hms_opt(0, 0, 0) {
            if let Some(local) = Local.from_local_datetime(&dt).earliest() {
                starts[i] = local.timestamp();
            }
        }
    }
    (starts, starts[0])
}

/// Bucket a unix timestamp into the 7-day window, returning the day index (0-6)
/// or None if outside the window.
fn bucket_timestamp(ts: i64, day_starts: &[i64; WINDOW_DAYS]) -> Option<usize> {
    for i in (0..WINDOW_DAYS).rev() {
        if ts >= day_starts[i] {
            return Some(i);
        }
    }
    None
}

/// Format a unix timestamp as ISO 8601.
fn ts_to_iso(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

/// Parse an ISO 8601 string to a unix timestamp.
fn iso_to_ts(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp())
        .or_else(|| {
            // Try parsing without timezone (some providers omit it)
            NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
                .ok()
                .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok())
                .and_then(|ndt| Local.from_local_datetime(&ndt).earliest())
                .map(|dt| dt.timestamp())
        })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

// -- Per-provider scanners --
// Each returns Vec<(canonical_path, activity)> and never panics.

fn scan_claude_code(day_starts: &[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)> {
    let inner = || -> Option<Vec<(String, DiscoveredProviderActivity)>> {
        let projects_dir = home_dir()?.join(".claude/projects");
        if !projects_dir.is_dir() {
            return None;
        }

        let mut results = Vec::new();

        for entry in fs::read_dir(&projects_dir).ok()? {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();
            let Some(path) = unmunge_claude_path(&dir_name) else {
                continue;
            };

            // Stat all .jsonl files for timestamps
            let mut max_ts: i64 = 0;
            let mut daily = [0u32; WINDOW_DAYS];

            if let Ok(files) = fs::read_dir(entry.path()) {
                for file in files.flatten() {
                    let fname = file.file_name();
                    if !fname.to_string_lossy().ends_with(".jsonl") {
                        continue;
                    }
                    if let Ok(meta) = file.metadata() {
                        if let Ok(mtime) = meta.modified() {
                            let ts = mtime
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            if ts > max_ts {
                                max_ts = ts;
                            }
                            if let Some(idx) = bucket_timestamp(ts, day_starts) {
                                daily[idx] += 1;
                            }
                        }
                    }
                }
            }

            if max_ts > 0 {
                results.push((
                    path,
                    DiscoveredProviderActivity {
                        provider_id: PROVIDER_CLAUDE_CODE.into(),
                        last_active: ts_to_iso(max_ts),
                        daily_counts: daily,
                    },
                ));
            }
        }

        Some(results)
    };

    inner().unwrap_or_default()
}

/// Unmunge a Claude Code project directory name back to an absolute path.
///
/// Claude encodes path separators, dots, and hyphens all as `-`, making
/// the encoding ambiguous. For example `-repo-Nix-mix-nix` could decode
/// to `/repo/Nix/mix/nix`, `/repo/Nix/mix.nix`, or `/repo/Nix/mix-nix`.
///
/// We resolve this by walking the filesystem: at each `-` in the encoded
/// name, we try interpreting it as `/` (path separator), `.` (dot in name),
/// or literal `-`, and keep only interpretations that match existing dirs.
fn unmunge_claude_path(dir_name: &str) -> Option<String> {
    if !dir_name.starts_with('-') {
        return None;
    }

    // Split on `-`, first element is empty (from leading `-`)
    let segments: Vec<&str> = dir_name.split('-').collect();
    if segments.len() < 2 {
        return None;
    }

    // segments[0] is "" (before the leading -), segments[1..] are the parts
    let parts: Vec<&str> = segments[1..]
        .iter()
        .copied()
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }

    let mut candidates = vec![String::from("/")];

    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            // First segment always joins with `/`
            candidates = candidates
                .into_iter()
                .map(|c| format!("{}{}", c, part))
                .collect();
            continue;
        }

        let mut next_candidates = Vec::new();

        for candidate in &candidates {
            // Try three interpretations of this `-`:
            // 1. Path separator: candidate + "/" + part
            let as_slash = format!("{}/{}", candidate, part);
            // 2. Dot in filename: candidate + "." + part
            let as_dot = format!("{}.{}", candidate, part);
            // 3. Literal hyphen: candidate + "-" + part
            let as_hyphen = format!("{}-{}", candidate, part);

            // Keep interpretations where the path exists as a directory
            if Path::new(&as_slash).is_dir() {
                next_candidates.push(as_slash);
            }
            if Path::new(&as_dot).is_dir() {
                next_candidates.push(as_dot);
            }
            if Path::new(&as_hyphen).is_dir() {
                next_candidates.push(as_hyphen);
            }
        }

        if next_candidates.is_empty() {
            // No valid path found at this level — fall back to all-slash
            // interpretation for remaining segments and let path_exists
            // handle it downstream
            candidates = candidates
                .into_iter()
                .map(|c| format!("{}/{}", c, part))
                .collect();
        } else {
            candidates = next_candidates;
        }

        // Cap candidates to prevent exponential blowup from ambiguous segments
        candidates.truncate(16);
    }

    // Prefer the first candidate (typically the correct match)
    candidates.into_iter().next()
}

fn scan_codex(day_starts: &[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)> {
    let inner = || -> Option<Vec<(String, DiscoveredProviderActivity)>> {
        let db_path = home_dir()?.join(".codex/state_5.sqlite");
        if !db_path.exists() {
            return None;
        }

        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;

        // Codex stores updated_at as unix epoch milliseconds
        let mut stmt = conn
            .prepare(
                "SELECT cwd, created_at, updated_at
                 FROM threads
                 WHERE archived = 0 AND cwd IS NOT NULL AND cwd != ''",
            )
            .ok()?;

        // Group by cwd
        let mut by_cwd: HashMap<String, (i64, [u32; WINDOW_DAYS])> = HashMap::new();

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .ok()?;

        for row in rows.flatten() {
            let (cwd, _created, updated) = row;
            // Codex timestamps are milliseconds
            let ts = updated / 1000;
            let entry = by_cwd.entry(cwd).or_insert((0, [0u32; WINDOW_DAYS]));
            if ts > entry.0 {
                entry.0 = ts;
            }
            if let Some(idx) = bucket_timestamp(ts, day_starts) {
                entry.1[idx] += 1;
            }
        }

        let results = by_cwd
            .into_iter()
            .filter(|(_, (max_ts, _))| *max_ts > 0)
            .map(|(cwd, (max_ts, daily))| {
                (
                    cwd,
                    DiscoveredProviderActivity {
                        provider_id: PROVIDER_CODEX.into(),
                        last_active: ts_to_iso(max_ts),
                        daily_counts: daily,
                    },
                )
            })
            .collect();

        Some(results)
    };

    inner().unwrap_or_default()
}

fn scan_copilot(day_starts: &[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)> {
    let inner = || -> Option<Vec<(String, DiscoveredProviderActivity)>> {
        let session_dir = home_dir()?.join(".copilot/session-state");
        if !session_dir.is_dir() {
            return None;
        }

        let mut by_cwd: HashMap<String, (i64, [u32; WINDOW_DAYS])> = HashMap::new();

        for entry in fs::read_dir(&session_dir).ok()?.flatten() {
            if !entry.file_type().ok().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }

            let yaml_path = entry.path().join("workspace.yaml");
            if !yaml_path.exists() {
                continue;
            }

            let Ok(content) = fs::read_to_string(&yaml_path) else {
                continue;
            };
            if let Some((cwd, updated_at)) = parse_copilot_workspace_yaml(&content) {
                let ts = iso_to_ts(&updated_at).unwrap_or_else(|| {
                    // Fall back to file mtime
                    fs::metadata(&yaml_path)
                        .and_then(|m| m.modified())
                        .map(|t| {
                            t.duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64
                        })
                        .unwrap_or(0)
                });

                let entry = by_cwd.entry(cwd).or_insert((0, [0u32; WINDOW_DAYS]));
                if ts > entry.0 {
                    entry.0 = ts;
                }
                if let Some(idx) = bucket_timestamp(ts, day_starts) {
                    entry.1[idx] += 1;
                }
            }
        }

        let results = by_cwd
            .into_iter()
            .filter(|(_, (max_ts, _))| *max_ts > 0)
            .map(|(cwd, (max_ts, daily))| {
                (
                    cwd,
                    DiscoveredProviderActivity {
                        provider_id: PROVIDER_COPILOT.into(),
                        last_active: ts_to_iso(max_ts),
                        daily_counts: daily,
                    },
                )
            })
            .collect();

        Some(results)
    };

    inner().unwrap_or_default()
}

/// Minimal line-scan of Copilot workspace.yaml to extract cwd and updated_at
/// without a YAML parser dependency.
fn parse_copilot_workspace_yaml(content: &str) -> Option<(String, String)> {
    let mut cwd = None;
    let mut updated_at = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(val) = trimmed.strip_prefix("cwd:") {
            cwd = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
        if let Some(val) = trimmed.strip_prefix("updated_at:") {
            updated_at = Some(val.trim().trim_matches('"').trim_matches('\'').to_string());
        }
    }

    Some((cwd?, updated_at.unwrap_or_default()))
}

fn scan_gemini(day_starts: &[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)> {
    let inner = || -> Option<Vec<(String, DiscoveredProviderActivity)>> {
        let gemini_dir = home_dir()?.join(".gemini");
        let projects_json = gemini_dir.join("projects.json");
        if !projects_json.exists() {
            return None;
        }

        let content = fs::read_to_string(&projects_json).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let projects = parsed.get("projects")?.as_object()?;

        let mut results = Vec::new();

        for (abs_path, short_name) in projects {
            let short = short_name.as_str().unwrap_or("");

            // Check both tmp/<name>/chats/ and history/<name>/
            let mut max_ts: i64 = 0;
            let mut daily = [0u32; WINDOW_DAYS];

            let chats_dir = gemini_dir.join("tmp").join(short).join("chats");
            if chats_dir.is_dir() {
                if let Ok(files) = fs::read_dir(&chats_dir) {
                    for file in files.flatten() {
                        if let Ok(meta) = file.metadata() {
                            if let Ok(mtime) = meta.modified() {
                                let ts = mtime
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                if ts > max_ts {
                                    max_ts = ts;
                                }
                                if let Some(idx) = bucket_timestamp(ts, day_starts) {
                                    daily[idx] += 1;
                                }
                            }
                        }
                    }
                }
            }

            if max_ts > 0 {
                results.push((
                    abs_path.clone(),
                    DiscoveredProviderActivity {
                        provider_id: PROVIDER_GEMINI.into(),
                        last_active: ts_to_iso(max_ts),
                        daily_counts: daily,
                    },
                ));
            }
        }

        Some(results)
    };

    inner().unwrap_or_default()
}

fn scan_crush(day_starts: &[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)> {
    let inner = || -> Option<Vec<(String, DiscoveredProviderActivity)>> {
        let json_path = home_dir()?.join(".local/share/crush/projects.json");
        if !json_path.exists() {
            return None;
        }

        let content = fs::read_to_string(&json_path).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let projects = parsed.get("projects")?.as_array()?;

        let mut results = Vec::new();

        for project in projects {
            let path = project.get("path")?.as_str()?;
            let last_accessed = project.get("last_accessed").and_then(|v| v.as_str());

            let ts = last_accessed.and_then(iso_to_ts).unwrap_or(0);

            if ts > 0 {
                let mut daily = [0u32; WINDOW_DAYS];
                if let Some(idx) = bucket_timestamp(ts, day_starts) {
                    daily[idx] += 1;
                }

                results.push((
                    path.to_string(),
                    DiscoveredProviderActivity {
                        provider_id: PROVIDER_CRUSH.into(),
                        last_active: ts_to_iso(ts),
                        daily_counts: daily,
                    },
                ));
            }
        }

        Some(results)
    };

    inner().unwrap_or_default()
}

fn scan_opencode(day_starts: &[i64; WINDOW_DAYS]) -> Vec<(String, DiscoveredProviderActivity)> {
    let inner = || -> Option<Vec<(String, DiscoveredProviderActivity)>> {
        let db_path = home_dir()?.join(".local/share/opencode/opencode.db");
        if !db_path.exists() {
            return None;
        }

        let conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()?;

        // OpenCode stores time_updated as epoch milliseconds
        let mut stmt = conn
            .prepare(
                "SELECT p.worktree, s.time_updated
                 FROM project p
                 LEFT JOIN session s ON s.project_id = p.id
                 WHERE p.worktree IS NOT NULL AND p.worktree != ''",
            )
            .ok()?;

        let mut by_path: HashMap<String, (i64, [u32; WINDOW_DAYS])> = HashMap::new();

        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })
            .ok()?;

        for row in rows.flatten() {
            let (worktree, time_updated) = row;
            let ts = time_updated.unwrap_or(0) / 1000;

            let entry = by_path.entry(worktree).or_insert((0, [0u32; WINDOW_DAYS]));
            if ts > entry.0 {
                entry.0 = ts;
            }
            if let Some(idx) = bucket_timestamp(ts, day_starts) {
                entry.1[idx] += 1;
            }
        }

        let results = by_path
            .into_iter()
            .filter(|(_, (max_ts, _))| *max_ts > 0)
            .map(|(path, (max_ts, daily))| {
                (
                    path,
                    DiscoveredProviderActivity {
                        provider_id: PROVIDER_OPENCODE.into(),
                        last_active: ts_to_iso(max_ts),
                        daily_counts: daily,
                    },
                )
            })
            .collect();

        Some(results)
    };

    inner().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmunge_simple_path() {
        // /home always exists on Linux
        let result = unmunge_claude_path("-home");
        assert_eq!(result, Some("/home".into()));
    }

    #[test]
    fn unmunge_nested_path() {
        // /home/toph should resolve through filesystem walking
        let result = unmunge_claude_path("-home-toph");
        // The walk will find /home exists, then /home/toph exists
        assert!(result.is_some());
        let path = result.unwrap();
        assert!(path.starts_with("/home"));
    }

    #[test]
    fn unmunge_rejects_no_leading_dash() {
        assert_eq!(unmunge_claude_path("home-toph"), None);
    }

    #[test]
    fn unmunge_rejects_empty() {
        assert_eq!(unmunge_claude_path(""), None);
        // `-` alone has no segments after the leading dash
        assert_eq!(unmunge_claude_path("-"), None);
    }

    #[test]
    fn unmunge_resolves_dot_in_name() {
        // `-repo-Nix-mix-nix` should find `/repo/Nix/mix.nix` if it exists,
        // not `/repo/Nix/mix/nix`. This tests the path-walking disambiguation.
        let result = unmunge_claude_path("-repo-Nix-mix-nix");
        if let Some(ref path) = result {
            // If /repo/Nix/mix.nix exists, it should be preferred
            if Path::new("/repo/Nix/mix.nix").is_dir() {
                assert_eq!(path, "/repo/Nix/mix.nix");
            }
        }
    }

    #[test]
    fn unmunge_resolves_hyphen_in_name() {
        // `-repo-Nix-niri-flake` should find `/repo/Nix/niri-flake`
        let result = unmunge_claude_path("-repo-Nix-niri-flake");
        if let Some(ref path) = result {
            if Path::new("/repo/Nix/niri-flake").is_dir() {
                assert_eq!(path, "/repo/Nix/niri-flake");
            }
        }
    }

    #[test]
    fn bucket_timestamp_edges() {
        let (starts, _) = day_boundaries();
        // A timestamp before the window returns None
        assert_eq!(bucket_timestamp(0, &starts), None);
        // A timestamp at the last day boundary returns index 6
        assert_eq!(bucket_timestamp(starts[6], &starts), Some(6));
    }
}
