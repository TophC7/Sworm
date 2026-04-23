// Per-project task loading, variable substitution, and file watching.
//
// Tasks live in `<project>/.sworm/tasks.json`. This service reads them,
// resolves `${workspaceFolder}`, `${file}`, and `${env:NAME}` variables
// against the current project context, and watches the file so the
// frontend can refresh its palette/menu listings when it changes.
//
// The PTY spawn itself is driven by commands (see commands/tasks.rs);
// this service is purely config loading and bookkeeping.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use tauri::Emitter;
use tracing::warn;

use crate::models::task::{TaskDefinition, TasksFile};

pub const TASKS_FILE_REL: &str = ".sworm/tasks.json";
pub const TASKS_CHANGED_EVENT: &str = "tasks-changed";

/// A task with its command, cwd, and env fully resolved for spawn.
pub struct ResolvedTask {
    pub command: String,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
}

struct ProjectWatcher {
    watcher: RecommendedWatcher,
    watching_sworm_dir: bool,
}

pub struct TaskService {
    /// One filesystem watcher per project. Repeated `watch` calls are
    /// idempotent and may upgrade an existing project-root watcher to
    /// also watch `.sworm/` once that directory exists.
    watchers: Mutex<HashMap<String, ProjectWatcher>>,
}

impl TaskService {
    pub fn new() -> Self {
        Self {
            watchers: Mutex::new(HashMap::new()),
        }
    }

    /// Parse `.sworm/tasks.json` from the given project path. Returns
    /// an empty list when the file is absent so callers don't need to
    /// distinguish "no config" from "empty config".
    pub fn load(&self, project_path: &Path) -> Result<Vec<TaskDefinition>, String> {
        let path = project_path.join(TASKS_FILE_REL);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let parsed: TasksFile =
            serde_json::from_str(&content).map_err(|e| format!("Invalid tasks.json: {}", e))?;
        Ok(parsed.tasks)
    }

    /// Find one task by id.
    pub fn find(
        &self,
        project_path: &Path,
        task_id: &str,
    ) -> Result<Option<TaskDefinition>, String> {
        Ok(self
            .load(project_path)?
            .into_iter()
            .find(|t| t.id == task_id))
    }

    /// Resolve a task's command / cwd / env for PTY spawn.
    ///
    /// Variables substituted:
    /// <> `${workspaceFolder}` = project root
    /// <> `${file}` = active editor file (blank if none)
    /// <> `${env:NAME}` = value from `base_env` or process env
    pub fn resolve(
        &self,
        task: &TaskDefinition,
        project_path: &Path,
        active_file: Option<&str>,
        base_env: &HashMap<String, String>,
    ) -> ResolvedTask {
        let command = substitute_vars(&task.command, project_path, active_file, base_env);

        let cwd = match &task.cwd {
            Some(rel) => {
                let expanded = substitute_vars(rel, project_path, active_file, base_env);
                let p = Path::new(&expanded);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    project_path.join(expanded)
                }
            }
            None => project_path.to_path_buf(),
        };

        let mut env: HashMap<String, String> = base_env.clone();
        if let Some(extras) = &task.env {
            for (k, v) in extras {
                env.insert(
                    k.clone(),
                    substitute_vars(v, project_path, active_file, base_env),
                );
            }
        }

        ResolvedTask { command, cwd, env }
    }

    /// Start watching task config paths for this project. Always
    /// watches the project root for `.sworm/` creation and upgrades to
    /// also watch `.sworm/` directly once it exists. Safe to call on
    /// every `tasks_list`.
    pub fn watch(
        &self,
        app: &tauri::AppHandle,
        project_id: &str,
        project_path: &Path,
    ) -> Result<(), String> {
        let mut watchers = self.watchers.lock();
        let sworm_dir = project_path.join(".sworm");
        if let Some(project_watcher) = watchers.get_mut(project_id) {
            if !sworm_dir.exists() {
                project_watcher.watching_sworm_dir = false;
            }
            ensure_sworm_watch(project_watcher, &sworm_dir)?;
            return Ok(());
        }

        let tasks_file = project_path.join(TASKS_FILE_REL);
        let handle = app.clone();
        let pid = project_id.to_string();
        let sworm_dir_for_events = sworm_dir.clone();
        let tasks_file_for_events = tasks_file.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if event.paths.iter().any(|path| {
                is_tasks_event_path(path, &sworm_dir_for_events, &tasks_file_for_events)
            }) {
                let _ = handle.emit(TASKS_CHANGED_EVENT, &pid);
            }
        })
        .map_err(|e| format!("Failed to create watcher: {}", e))?;

        watcher
            .watch(project_path, RecursiveMode::NonRecursive)
            .map_err(|e| format!("Failed to watch project root: {}", e))?;

        let mut project_watcher = ProjectWatcher {
            watcher,
            watching_sworm_dir: false,
        };
        ensure_sworm_watch(&mut project_watcher, &sworm_dir)?;

        watchers.insert(project_id.to_string(), project_watcher);
        Ok(())
    }

    /// Drop the watcher for a project. Called when a project is removed.
    #[allow(dead_code)]
    pub fn unwatch(&self, project_id: &str) {
        self.watchers.lock().remove(project_id);
    }
}

fn ensure_sworm_watch(watcher: &mut ProjectWatcher, sworm_dir: &Path) -> Result<(), String> {
    if watcher.watching_sworm_dir || !sworm_dir.exists() {
        return Ok(());
    }

    watcher
        .watcher
        .watch(sworm_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to watch .sworm/: {}", e))?;
    watcher.watching_sworm_dir = true;
    Ok(())
}

fn is_tasks_event_path(path: &Path, sworm_dir: &Path, tasks_file: &Path) -> bool {
    path == sworm_dir
        || path == tasks_file
        || (path.parent() == Some(sworm_dir)
            && path.file_name().is_some_and(|name| name == "tasks.json"))
}

fn substitute_vars(
    input: &str,
    project_path: &Path,
    active_file: Option<&str>,
    base_env: &HashMap<String, String>,
) -> String {
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;
    let bytes = input.as_bytes();

    while cursor < bytes.len() {
        // Look for `${`
        if cursor + 1 < bytes.len() && bytes[cursor] == b'$' && bytes[cursor + 1] == b'{' {
            if let Some(end_rel) = input[cursor + 2..].find('}') {
                let key_start = cursor + 2;
                let key_end = cursor + 2 + end_rel;
                let key = &input[key_start..key_end];
                match resolve_var(key, project_path, active_file, base_env) {
                    Some(value) => out.push_str(&value),
                    None => {
                        // Unknown variable: leave the literal in place so
                        // a missed typo is visible rather than silently
                        // expanding to an empty string.
                        out.push_str(&input[cursor..=key_end]);
                    }
                }
                cursor = key_end + 1;
                continue;
            }
        }
        // Regular char. Use str slicing to stay UTF-8 safe.
        let ch = input[cursor..].chars().next().unwrap();
        out.push(ch);
        cursor += ch.len_utf8();
    }

    out
}

fn resolve_var(
    key: &str,
    project_path: &Path,
    active_file: Option<&str>,
    base_env: &HashMap<String, String>,
) -> Option<String> {
    match key {
        "workspaceFolder" => Some(project_path.to_string_lossy().into_owned()),
        "file" => Some(active_file.unwrap_or("").to_string()),
        _ => {
            if let Some(name) = key.strip_prefix("env:") {
                if let Some(val) = base_env.get(name) {
                    return Some(val.clone());
                }
                if let Ok(val) = std::env::var(name) {
                    return Some(val);
                }
                // Missing env var: expand to empty rather than leave
                // the literal, matching typical shell behavior.
                return Some(String::new());
            }
            warn!("Unknown task variable: ${{{}}}", key);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_env() -> HashMap<String, String> {
        HashMap::new()
    }

    #[test]
    fn substitutes_workspace_folder() {
        let out = substitute_vars(
            "cd ${workspaceFolder}",
            Path::new("/repo"),
            None,
            &empty_env(),
        );
        assert_eq!(out, "cd /repo");
    }

    #[test]
    fn substitutes_file_with_fallback_to_empty() {
        let out = substitute_vars("bun test ${file}", Path::new("/repo"), None, &empty_env());
        assert_eq!(out, "bun test ");
    }

    #[test]
    fn substitutes_env_from_base() {
        let mut env = empty_env();
        env.insert("FOO".into(), "bar".into());
        let out = substitute_vars("echo ${env:FOO}", Path::new("/repo"), None, &env);
        assert_eq!(out, "echo bar");
    }

    #[test]
    fn leaves_unknown_vars_literal() {
        let out = substitute_vars("${mystery}", Path::new("/repo"), None, &empty_env());
        assert_eq!(out, "${mystery}");
    }

    #[test]
    fn handles_multiple_vars() {
        let out = substitute_vars(
            "${workspaceFolder}/src ${file}",
            Path::new("/repo"),
            Some("a.ts"),
            &empty_env(),
        );
        assert_eq!(out, "/repo/src a.ts");
    }

    #[test]
    fn matches_sworm_directory_events() {
        assert!(is_tasks_event_path(
            Path::new("/repo/.sworm"),
            Path::new("/repo/.sworm"),
            Path::new("/repo/.sworm/tasks.json"),
        ));
    }

    #[test]
    fn matches_tasks_file_events() {
        assert!(is_tasks_event_path(
            Path::new("/repo/.sworm/tasks.json"),
            Path::new("/repo/.sworm"),
            Path::new("/repo/.sworm/tasks.json"),
        ));
    }

    #[test]
    fn ignores_unrelated_paths() {
        assert!(!is_tasks_event_path(
            Path::new("/repo/src/main.rs"),
            Path::new("/repo/.sworm"),
            Path::new("/repo/.sworm/tasks.json"),
        ));
    }
}
