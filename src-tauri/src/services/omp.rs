//! OMP session-store lookups.
//!
//! OMP keeps every session under `~/.omp/agent/sessions/<bucket>/`, one
//! bucket per working directory, as `<ts>_<id>.jsonl` where `ts` is
//! `YYYY-MM-DDTHH-MM-SS-mmmZ` (UTC). Sworm never owns OMP state; it only
//! reads this layout to validate and discover resume ids.

use crate::errors::ApiError;
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct OmpResolvedTarget {
    pub path: String,
    pub is_dir: bool,
}

fn latest_session_paths(cwd: &str) -> Option<(PathBuf, PathBuf)> {
    let bucket = session_bucket(cwd)?;
    let entries = std::fs::read_dir(&bucket).ok()?;
    let latest = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".jsonl") {
                Some(name)
            } else {
                None
            }
        })
        .max()?;
    let stem = latest.strip_suffix(".jsonl")?;
    let jsonl_file = bucket.join(&latest);
    let artifacts_dir = bucket.join(stem);
    Some((jsonl_file, artifacts_dir))
}

fn find_any_latest_session() -> Option<(PathBuf, PathBuf)> {
    let home = home_dir()?;
    let sessions_root = home.join(".omp").join("agent").join("sessions");
    let bucket_entries = std::fs::read_dir(&sessions_root).ok()?;
    let mut best: Option<(String, PathBuf, PathBuf)> = None;
    for b in bucket_entries.flatten() {
        let bpath = b.path();
        if bpath.is_dir() {
            if let Ok(files) = std::fs::read_dir(&bpath) {
                for f in files.flatten() {
                    let name = f.file_name().to_string_lossy().into_owned();
                    if name.ends_with(".jsonl") {
                        if best.as_ref().map_or(true, |(prev, _, _)| name > *prev) {
                            let full_file = bpath.join(&name);
                            best = Some((name, full_file, bpath.clone()));
                        }
                    }
                }
            }
        }
    }
    let (_, jsonl_file, bucket_path) = best?;
    let file_name = jsonl_file.file_name()?.to_string_lossy();
    let stem = file_name.strip_suffix(".jsonl")?;
    let artifacts_dir = bucket_path.join(stem);
    Some((jsonl_file, artifacts_dir))
}

fn fallback_omp_read(uri: &str, home: &Path, slug: &str) -> Result<OmpResolvedTarget, ApiError> {
    let cache_dir = home.join(".cache").join("sworm").join("omp-docs");
    let safe_slug = slug.replace(['/', ':', ' '], "-");
    let doc_file = cache_dir.join(format!("{safe_slug}.md"));
    if doc_file.is_file() {
        return Ok(OmpResolvedTarget {
            path: doc_file.to_string_lossy().into_owned(),
            is_dir: false,
        });
    }
    let output = std::process::Command::new("omp")
        .args(["read", uri])
        .output()
        .map_err(|e| ApiError::Io(format!("Failed to run omp read: {e}")))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        let msg = if !err.trim().is_empty() { err } else { out };
        return Err(ApiError::Internal(msg.trim().to_string()));
    }
    let text = String::from_utf8(output.stdout).map_err(|e| ApiError::Internal(e.to_string()))?;
    std::fs::create_dir_all(&cache_dir)?;
    std::fs::write(&doc_file, text)?;
    Ok(OmpResolvedTarget {
        path: doc_file.to_string_lossy().into_owned(),
        is_dir: false,
    })
}

pub fn resolve_omp_target(uri: &str, cwd: Option<&str>) -> Result<OmpResolvedTarget, ApiError> {
    let home =
        home_dir().ok_or_else(|| ApiError::NotFound("HOME directory not set".to_string()))?;
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| ApiError::InvalidArgument(format!("Invalid URI scheme: {uri}")))?;
    let target_raw = rest.split('?').next().unwrap_or(rest);
    let target_base = target_raw.split(':').next().unwrap_or(target_raw);
    let target = target_base.trim_matches('/');

    let get_session = || {
        cwd.and_then(latest_session_paths)
            .or_else(find_any_latest_session)
    };

    match scheme {
        "history" => {
            if let Some((jsonl_file, artifacts_dir)) = get_session() {
                if target.is_empty() || target == "Main" {
                    return Ok(OmpResolvedTarget {
                        path: jsonl_file.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                let subagent_file = artifacts_dir.join(format!("{target}.jsonl"));
                if subagent_file.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: subagent_file.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                return Ok(OmpResolvedTarget {
                    path: jsonl_file.to_string_lossy().into_owned(),
                    is_dir: false,
                });
            }
            fallback_omp_read(uri, &home, target)
        }
        "agent" => {
            if let Some((jsonl_file, artifacts_dir)) = get_session() {
                if target.is_empty() || target == "Main" {
                    return Ok(OmpResolvedTarget {
                        path: jsonl_file.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                let candidate = artifacts_dir.join(format!("{target}.md"));
                if candidate.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: candidate.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
            }
            let user_agent = home
                .join(".omp")
                .join("agent")
                .join("agents")
                .join(format!("{target}.md"));
            if user_agent.is_file() {
                return Ok(OmpResolvedTarget {
                    path: user_agent.to_string_lossy().into_owned(),
                    is_dir: false,
                });
            }
            if let Some(c) = cwd {
                let proj_agent = Path::new(c)
                    .join("omp")
                    .join("agents")
                    .join(format!("{target}.md"));
                if proj_agent.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: proj_agent.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                let proj_agent_hidden = Path::new(c)
                    .join(".omp")
                    .join("agents")
                    .join(format!("{target}.md"));
                if proj_agent_hidden.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: proj_agent_hidden.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
            }
            fallback_omp_read(uri, &home, target)
        }
        "artifact" => {
            if let Some((_, artifacts_dir)) = get_session() {
                if artifacts_dir.is_dir() {
                    if let Ok(entries) = std::fs::read_dir(&artifacts_dir) {
                        let mut best: Option<(u32, PathBuf)> = None;
                        for e in entries.flatten() {
                            let name = e.file_name().to_string_lossy().into_owned();
                            if target == "latest" {
                                if let Some(first) = name.split('.').next() {
                                    if let Ok(num) = first.parse::<u32>() {
                                        if best
                                            .as_ref()
                                            .map_or(true, |(prev_num, _)| num > *prev_num)
                                        {
                                            best = Some((num, artifacts_dir.join(&name)));
                                        }
                                    }
                                }
                            } else if name.starts_with(&format!("{target}.")) || name == target {
                                return Ok(OmpResolvedTarget {
                                    path: artifacts_dir.join(&name).to_string_lossy().into_owned(),
                                    is_dir: false,
                                });
                            }
                        }
                        if let Some((_, file)) = best {
                            return Ok(OmpResolvedTarget {
                                path: file.to_string_lossy().into_owned(),
                                is_dir: false,
                            });
                        }
                    }
                }
            }
            fallback_omp_read(uri, &home, target)
        }
        "local" => {
            if let Some((_, artifacts_dir)) = get_session() {
                let cand1 = artifacts_dir.join("local").join(target);
                if cand1.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: cand1.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                let cand2 = artifacts_dir.join(target);
                if cand2.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: cand2.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
            }
            if let Some(c) = cwd {
                let cand3 = Path::new(c).join(".omp").join(target);
                if cand3.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: cand3.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                let cand4 = Path::new(c).join(target);
                if cand4.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: cand4.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
            }
            fallback_omp_read(uri, &home, target)
        }
        "skill" => {
            let (skill_name, subpath) = target.split_once('/').unwrap_or((target, "SKILL.md"));
            let rel = if subpath.is_empty() {
                "SKILL.md"
            } else {
                subpath
            };
            let plugins_dir = home.join(".omp").join("plugins").join("node_modules");
            if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
                for entry in entries.flatten() {
                    let cand = entry.path().join("skills").join(skill_name).join(rel);
                    if cand.exists() {
                        return Ok(OmpResolvedTarget {
                            is_dir: cand.is_dir(),
                            path: cand.to_string_lossy().into_owned(),
                        });
                    }
                }
            }
            let user_skill = home
                .join(".omp")
                .join("agent")
                .join("skills")
                .join(skill_name)
                .join(rel);
            if user_skill.exists() {
                return Ok(OmpResolvedTarget {
                    is_dir: user_skill.is_dir(),
                    path: user_skill.to_string_lossy().into_owned(),
                });
            }
            if let Some(c) = cwd {
                let proj_skill = Path::new(c)
                    .join(".omp")
                    .join("skills")
                    .join(skill_name)
                    .join(rel);
                if proj_skill.exists() {
                    return Ok(OmpResolvedTarget {
                        is_dir: proj_skill.is_dir(),
                        path: proj_skill.to_string_lossy().into_owned(),
                    });
                }
            }
            fallback_omp_read(uri, &home, target)
        }
        "rule" => {
            if let Some(c) = cwd {
                let cand1 = Path::new(c)
                    .join("omp")
                    .join("extensions")
                    .join(format!("{target}.ts"));
                if cand1.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: cand1.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
                let cand2 = Path::new(c)
                    .join(".omp")
                    .join("rules")
                    .join(format!("{target}.md"));
                if cand2.is_file() {
                    return Ok(OmpResolvedTarget {
                        path: cand2.to_string_lossy().into_owned(),
                        is_dir: false,
                    });
                }
            }
            let user_ext = home
                .join(".omp")
                .join("agent")
                .join("extensions")
                .join(format!("{target}.ts"));
            if user_ext.is_file() {
                return Ok(OmpResolvedTarget {
                    path: user_ext.to_string_lossy().into_owned(),
                    is_dir: false,
                });
            }
            let user_rule = home
                .join(".omp")
                .join("agent")
                .join("rules")
                .join(format!("{target}.md"));
            if user_rule.is_file() {
                return Ok(OmpResolvedTarget {
                    path: user_rule.to_string_lossy().into_owned(),
                    is_dir: false,
                });
            }
            fallback_omp_read(uri, &home, target)
        }
        "omp" => {
            let slug = if target.is_empty() { "index" } else { target };
            fallback_omp_read(uri, &home, slug)
        }
        _ => Err(ApiError::InvalidArgument(format!(
            "Unsupported scheme: {scheme}"
        ))),
    }
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
