//! OMP session-store lookups.
//!
//! OMP keeps every session under `~/.omp/agent/sessions/<bucket>/`, one
//! bucket per working directory, as `<ts>_<id>.jsonl` where `ts` is
//! `YYYY-MM-DDTHH-MM-SS-mmmZ` (UTC). Sworm never owns OMP state; it only
//! reads this layout to validate and discover resume ids.

use crate::services::folders::home_dir;
use chrono::{DateTime, NaiveDateTime, Utc};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Timestamp layout of OMP session file names; fixed width, so
/// lexicographic order equals chronological order.
const OMP_TS_FORMAT: &str = "%Y-%m-%dT%H-%M-%S-%3fZ";

/// Bucket directory name for `cwd`.
///
/// Folders under `$HOME` encode as `-` + the home-relative path with `/`
/// mapped to `-` (`~/Development/sworm` → `-Development-sworm`, `$HOME`
/// itself → `-`). Folders outside `$HOME` encode as `--` + the absolute
/// path without its leading `/` and `/` mapped to `-`, followed by `--`
/// (`/repo/Nix/omp.nix` → `--repo-Nix-omp.nix--`). Dots pass through.
fn bucket_name(cwd: &str, home: &Path) -> String {
    match Path::new(cwd).strip_prefix(home) {
        Ok(relative) => {
            let relative = relative.to_string_lossy();
            format!("-{}", relative.replace('/', "-"))
        }
        Err(_) => format!("--{}--", cwd.trim_start_matches('/').replace('/', "-")),
    }
}

/// `~/.omp/agent/sessions/<bucket>` for `cwd`; `None` when `$HOME` is unset.
pub fn session_bucket(cwd: &str) -> Option<PathBuf> {
    let home = home_dir()?;
    let bucket = bucket_name(cwd, &home);
    Some(
        home.join(".omp")
            .join("agent")
            .join("sessions")
            .join(bucket),
    )
}

/// Split an OMP session file name into `(ts, id)`; `None` for anything
/// that is not `<ts>_<id>.jsonl`.
fn parse_session_file_name(name: &str) -> Option<(&str, &str)> {
    let stem = name.strip_suffix(".jsonl")?;
    stem.split_once('_')
}

/// Visit sessions in `cwd`'s bucket created at or after `since`, oldest
/// first, excluding known ids. Returning `false` stops iteration.
///
/// OMP materializes the session file only after the first assistant
/// reply, so a fresh OMP run binds after its first exchange, not at start.
pub fn visit_sessions_created_since(
    cwd: &str,
    since: SystemTime,
    exclude: &HashSet<String>,
    mut visit: impl FnMut(SystemTime, String) -> bool,
) -> Option<()> {
    let entries = std::fs::read_dir(session_bucket(cwd)?).ok()?;
    let since_str = DateTime::<Utc>::from(since)
        .format(OMP_TS_FORMAT)
        .to_string();
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some((ts, id)) = name.to_str().and_then(parse_session_file_name) else {
            continue;
        };
        if ts < since_str.as_str() || exclude.contains(id) {
            continue;
        }
        let Ok(created_at) = NaiveDateTime::parse_from_str(ts, OMP_TS_FORMAT) else {
            continue;
        };
        candidates.push((
            ts.to_string(),
            id.to_string(),
            SystemTime::from(created_at.and_utc()),
        ));
    }
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    for (_, id, created_at) in candidates {
        if !visit(created_at, id) {
            break;
        }
    }
    Some(())
}

/// Whether a session file for `id` exists in `cwd`'s bucket.
pub fn session_exists(cwd: &str, id: &str) -> bool {
    let Some(entries) = session_bucket(cwd).and_then(|dir| std::fs::read_dir(dir).ok()) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .and_then(parse_session_file_name)
            .is_some_and(|(_, file_id)| file_id == id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Bucket names observed in a real ~/.omp/agent/sessions/.
    #[test]
    fn bucket_under_home() {
        let home = Path::new("/home/toph");
        assert_eq!(
            bucket_name("/home/toph/Development/sworm", home),
            "-Development-sworm"
        );
        assert_eq!(bucket_name("/home/toph", home), "-");
    }

    #[test]
    fn bucket_outside_home() {
        let home = Path::new("/home/toph");
        assert_eq!(
            bucket_name("/repo/Nix/omp.nix", home),
            "--repo-Nix-omp.nix--"
        );
    }

    #[test]
    fn session_file_name_splits_ts_and_id() {
        assert_eq!(
            parse_session_file_name(
                "2026-08-30T04-15-34-605Z_01a050e1-40cd-7104-abfb-204121a81217.jsonl"
            ),
            Some((
                "2026-08-30T04-15-34-605Z",
                "01a050e1-40cd-7104-abfb-204121a81217"
            ))
        );
        // Sibling directories share the stem but carry no extension.
        assert_eq!(
            parse_session_file_name(
                "2026-08-30T04-15-34-605Z_01a050e1-40cd-7104-abfb-204121a81217"
            ),
            None
        );
    }

    #[test]
    fn since_format_matches_file_name_layout() {
        let since = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_787_000_000_123);
        assert_eq!(
            DateTime::<Utc>::from(since)
                .format(OMP_TS_FORMAT)
                .to_string(),
            "2026-08-17T20-53-20-123Z"
        );
    }
}
