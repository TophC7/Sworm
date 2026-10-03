use crate::events::{deliver, EventSink, HostEvent};
use crate::services::settings::SettingsService;
use crate::services::settings_resolution::{
    parse_error_diagnostic, resolve_effective_settings_for_folder_path,
};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use sworm_protocol::settings::{SettingsChangedEvent, SettingsDiagnostic, SettingsLayerKind};

struct FolderSettingsWatcher {
    watcher: RecommendedWatcher,
    watching_sworm_dir: bool,
    directory_generation: Arc<AtomicU64>,
    registered_generation: u64,
}

pub struct SettingsWatcherService {
    events: EventSink<HostEvent>,
    generation: Arc<AtomicU64>,
    global_watcher: Mutex<Option<RecommendedWatcher>>,
    folder_watchers: Mutex<HashMap<PathBuf, Arc<Mutex<Option<FolderSettingsWatcher>>>>>,
}

impl SettingsWatcherService {
    pub fn new(events: EventSink<HostEvent>) -> Self {
        Self {
            events,
            generation: Arc::new(AtomicU64::new(0)),
            global_watcher: Mutex::new(None),
            folder_watchers: Mutex::new(HashMap::new()),
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn notify(&self, layer: SettingsLayerKind, folder: Option<&Path>) {
        emit_settings_changed(&self.events, &self.generation, layer, folder);
    }

    pub fn is_watching(&self, folder: &Path) -> bool {
        if !self
            .global_watcher
            .try_lock()
            .is_some_and(|slot| slot.is_some())
        {
            return false;
        }
        self.folder_watchers.lock().get(folder).is_some_and(|slot| {
            slot.try_lock().is_some_and(|watcher| {
                watcher.as_ref().is_some_and(|watcher| {
                    watcher.registered_generation
                        == watcher.directory_generation.load(Ordering::Acquire)
                })
            })
        })
    }

    pub fn watch_global(&self) -> Result<(), String> {
        let mut slot = self.global_watcher.lock();
        if slot.is_some() {
            return Ok(());
        }

        let settings_file = SettingsService::global_settings_path()?;
        let watch_path = existing_watch_parent(&settings_file)
            .ok_or_else(|| format!("No existing parent for {}", settings_file.display()))?;
        let events = Arc::clone(&self.events);
        let generation = Arc::clone(&self.generation);
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
            emit_settings_changed(&events, &generation, SettingsLayerKind::Global, None);
        })
        .map_err(|error| format!("Failed to create global settings watcher: {error}"))?;

        watcher
            .watch(&watch_path, RecursiveMode::Recursive)
            .map_err(|error| format!("Failed to watch {}: {error}", watch_path.display()))?;
        *slot = Some(watcher);
        Ok(())
    }

    pub fn watch_folder(&self, folder_path: &Path) -> Result<(), String> {
        let slot = Arc::clone(
            self.folder_watchers
                .lock()
                .entry(folder_path.to_path_buf())
                .or_default(),
        );
        let mut slot = slot.lock();
        if let Some(folder_watcher) = slot.as_mut() {
            let generation = folder_watcher.directory_generation.load(Ordering::Acquire);
            if folder_watcher.registered_generation == generation {
                return Ok(());
            }
            folder_watcher.watching_sworm_dir = false;
            ensure_sworm_watch(folder_watcher, &folder_path.join(".sworm"))?;
            folder_watcher.registered_generation = generation;
            return Ok(());
        }
        let folder_path = folder_path.to_path_buf();
        let sworm_dir = folder_path.join(".sworm");

        let settings_file = SettingsService::folder_settings_path(&folder_path);
        let events = Arc::clone(&self.events);
        let generation = Arc::clone(&self.generation);
        let settings_file_for_events = settings_file.clone();
        let folder_path_for_events = folder_path.clone();
        let directory_generation = Arc::new(AtomicU64::new(0));
        let watch_generation = Arc::clone(&directory_generation);
        let watched_dir = sworm_dir.clone();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if event.paths.iter().any(|path| path == &watched_dir) {
                // Deletion/recreation invalidates the old inotify watch even
                // when the directory exists again before the next request.
                watch_generation.fetch_add(1, Ordering::Release);
            }
            if !event
                .paths
                .iter()
                .any(|path| is_settings_event_path(path, &settings_file_for_events))
            {
                return;
            }
            emit_settings_changed(
                &events,
                &generation,
                SettingsLayerKind::Folder,
                Some(folder_path_for_events.as_path()),
            );
        })
        .map_err(|error| format!("Failed to create folder settings watcher: {error}"))?;

        watcher
            .watch(&folder_path, RecursiveMode::NonRecursive)
            .map_err(|error| format!("Failed to watch {}: {error}", folder_path.display()))?;

        let mut folder_watcher = FolderSettingsWatcher {
            watcher,
            watching_sworm_dir: false,
            directory_generation,
            registered_generation: 0,
        };
        ensure_sworm_watch(&mut folder_watcher, &sworm_dir)?;
        *slot = Some(folder_watcher);
        Ok(())
    }

    pub fn stop(&self, folder_path: &Path) {
        let watcher = self.folder_watchers.lock().remove(folder_path);
        drop(watcher);
    }
}

fn ensure_sworm_watch(watcher: &mut FolderSettingsWatcher, sworm_dir: &Path) -> Result<(), String> {
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
    path == settings_file || Some(path) == settings_file.parent()
}

fn emit_settings_changed(
    events: &EventSink<HostEvent>,
    generation: &AtomicU64,
    layer: SettingsLayerKind,
    folder_path: Option<&Path>,
) {
    let generation = generation.fetch_add(1, Ordering::AcqRel) + 1;
    let diagnostics = diagnostics_for(folder_path);
    deliver(
        events,
        HostEvent::SettingsChanged(SettingsChangedEvent {
            layer,
            folder_path: folder_path.map(|path| path.to_string_lossy().into_owned()),
            generation,
            diagnostics,
        }),
    );
}

fn diagnostics_for(folder_path: Option<&Path>) -> Vec<SettingsDiagnostic> {
    resolve_effective_settings_for_folder_path(folder_path)
        .map(|resolved| resolved.diagnostics)
        .unwrap_or_else(|message| {
            vec![parse_error_diagnostic(
                SettingsLayerKind::Global,
                Path::new(""),
                message,
            )]
        })
}
