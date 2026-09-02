use crate::commands::settings::SETTINGS_CHANGED_EVENT;
use crate::models::settings::{SettingsChangedEvent, SettingsDiagnostic, SettingsLayerKind};
use crate::services::settings::SettingsService;
use crate::services::settings_resolution::resolve_effective_settings_for_project_path;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;

struct ProjectSettingsWatcher {
    watcher: RecommendedWatcher,
    watching_sworm_dir: bool,
}

pub struct SettingsWatcherService {
    global_watcher: Mutex<Option<RecommendedWatcher>>,
    project_watchers: Mutex<HashMap<PathBuf, ProjectSettingsWatcher>>,
}

impl SettingsWatcherService {
    pub fn new() -> Self {
        Self {
            global_watcher: Mutex::new(None),
            project_watchers: Mutex::new(HashMap::new()),
        }
    }

    pub fn watch_global(
        &self,
        app: &tauri::AppHandle,
        generation: Arc<Mutex<u64>>,
    ) -> Result<(), String> {
        let mut slot = self.global_watcher.lock();
        if slot.is_some() {
            return Ok(());
        }

        let settings_file = SettingsService::global_settings_path()?;
        let watch_path = existing_watch_parent(&settings_file)
            .ok_or_else(|| format!("No existing parent for {}", settings_file.display()))?;
        let handle = app.clone();
        let settings_file_for_events = settings_file.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if !event
                .paths
                .iter()
                .any(|path| is_settings_event_path(path, &settings_file_for_events))
            {
                return;
            }
            emit_settings_changed(&handle, &generation, SettingsLayerKind::Global, None, None);
        })
        .map_err(|error| format!("Failed to create global settings watcher: {error}"))?;

        watcher
            .watch(&watch_path, RecursiveMode::Recursive)
            .map_err(|error| format!("Failed to watch {}: {error}", watch_path.display()))?;
        *slot = Some(watcher);
        Ok(())
    }

    pub fn watch_project(
        &self,
        app: &tauri::AppHandle,
        project_path: &Path,
        generation: Arc<Mutex<u64>>,
    ) -> Result<(), String> {
        let project_path = project_path.to_path_buf();
        let sworm_dir = project_path.join(".sworm");
        let mut watchers = self.project_watchers.lock();
        if let Some(project_watcher) = watchers.get_mut(&project_path) {
            if !sworm_dir.exists() {
                project_watcher.watching_sworm_dir = false;
            }
            ensure_sworm_watch(project_watcher, &sworm_dir)?;
            return Ok(());
        }

        let settings_file = SettingsService::project_settings_path(&project_path);
        let handle = app.clone();
        let settings_file_for_events = settings_file.clone();
        let project_path_for_events = project_path.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if !event
                .paths
                .iter()
                .any(|path| is_settings_event_path(path, &settings_file_for_events))
            {
                return;
            }
            emit_settings_changed(
                &handle,
                &generation,
                SettingsLayerKind::Project,
                Some(project_path_for_events.as_path()),
                None,
            );
        })
        .map_err(|error| format!("Failed to create project settings watcher: {error}"))?;

        watcher
            .watch(&project_path, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", project_path.display()))?;

        let mut project_watcher = ProjectSettingsWatcher {
            watcher,
            watching_sworm_dir: false,
        };
        ensure_sworm_watch(&mut project_watcher, &sworm_dir)?;
        watchers.insert(project_path, project_watcher);
        Ok(())
    }
}

fn ensure_sworm_watch(
    watcher: &mut ProjectSettingsWatcher,
    sworm_dir: &Path,
) -> Result<(), String> {
    if watcher.watching_sworm_dir || !sworm_dir.exists() {
        return Ok(());
    }

    watcher
        .watcher
        .watch(sworm_dir, RecursiveMode::NonRecursive)
        .map_err(|error| format!("Failed to watch .sworm/: {error}"))?;
    watcher.watching_sworm_dir = true;
    Ok(())
}

fn existing_watch_parent(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(candidate) = current {
        if candidate.exists() {
            return Some(candidate.to_path_buf());
        }
        current = candidate.parent();
    }
    None
}

fn is_settings_event_path(path: &Path, settings_file: &Path) -> bool {
    path == settings_file
        || (path.parent() == settings_file.parent()
            && path
                .file_name()
                .is_some_and(|name| name == "settings.jsonc"))
}

fn emit_settings_changed(
    app: &tauri::AppHandle,
    generation: &Arc<Mutex<u64>>,
    layer: SettingsLayerKind,
    project_path: Option<&Path>,
    diagnostics: Option<Vec<SettingsDiagnostic>>,
) {
    let diagnostics = diagnostics.unwrap_or_else(|| diagnostics_for(project_path));
    let generation = {
        let mut generation = generation.lock();
        *generation += 1;
        *generation
    };
    let _ = app.emit(
        SETTINGS_CHANGED_EVENT,
        SettingsChangedEvent {
            layer,
            folder_path: project_path.map(|p| p.to_string_lossy().into_owned()),
            generation,
            diagnostics,
        },
    );
}

fn diagnostics_for(project_path: Option<&Path>) -> Vec<SettingsDiagnostic> {
    resolve_effective_settings_for_project_path(project_path)
        .map(|resolved| resolved.diagnostics)
        .unwrap_or_else(|message| {
            vec![SettingsDiagnostic {
                layer: SettingsLayerKind::Global,
                path: String::new(),
                pointer: String::new(),
                code: crate::models::settings::SettingsDiagnosticCode::ParseError,
                severity: crate::models::settings::SettingsDiagnosticSeverity::Error,
                message,
            }]
        })
}
