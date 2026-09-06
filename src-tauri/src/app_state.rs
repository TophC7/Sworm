use crate::models::activity_map::DiscoveredProject;
use crate::services::{
    app_state_kv::AppStateKvService,
    db::{self, DatabaseService},
    env::EnvironmentService,
    file_watcher::FileWatcherService,
    files::FileService,
    git::GitService,
    git_watcher::GitWatcherService,
    issue_bridge::IssueBridgeService,
    issues::IssueService,
    lsp::LspService,
    providers::ProviderService,
    pty::PtyService,
    resume_discovery::ResumeDiscoveryService,
    settings_watcher::SettingsWatcherService,
    tasks::TaskService,
    windows::WindowCoordinatorService,
};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;

/// Central application state managed by Tauri.
///
/// Each service is behind a Mutex for safe concurrent access from
/// Tauri's async command handlers. PtyService manages its own
/// internal concurrency.
pub struct AppState {
    /// Database service. Internally manages a writer mutex + reader
    /// pool, so an outer `Mutex` would only re-serialize the reads we
    /// just split out.
    pub db: Arc<DatabaseService>,
    pub providers: ProviderService,
    pub pty: PtyService,
    /// Shared with the Git watcher so it can invalidate the summary cache.
    pub git: Arc<GitService>,
    pub issues: Arc<IssueService>,
    pub issue_bridge: IssueBridgeService,
    /// Shared so search walks can run on a `spawn_blocking` worker.
    pub files: Arc<FileService>,
    pub env: EnvironmentService,
    pub lsp: LspService,
    pub app_state_kv: AppStateKvService,
    pub windows: Arc<WindowCoordinatorService>,
    pub tasks: TaskService,
    pub settings_watchers: SettingsWatcherService,
    /// Watches the directories the file explorer currently renders.
    pub file_watchers: FileWatcherService,
    /// Watches each open folder's Git metadata and working tree.
    pub git_watchers: Arc<GitWatcherService>,
    /// Tracks folder paths with Nix evaluations in progress to prevent concurrent runs.
    pub nix_eval_locks: Mutex<HashSet<String>>,
    /// Post-spawn resume-token discovery for Codex/Antigravity/OMP runs.
    pub resume_discovery: ResumeDiscoveryService,
    /// Cached activity map scan results. None = not yet scanned.
    pub activity_map_cache: Mutex<Option<Vec<DiscoveredProject>>>,
    /// Monotonic generation for settings file writes and watcher events.
    pub settings_generation: Arc<Mutex<u64>>,
}

impl AppState {
    /// Initialize all services. Database migrations run automatically.
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = db::resolve_db_path(app_handle)?;
        let db_service = Arc::new(DatabaseService::new(db_path)?);
        let issues = Arc::new(IssueService::new());
        let git = Arc::new(GitService::new());

        Ok(Self {
            db: db_service,
            providers: ProviderService,
            pty: PtyService::new(),
            git: Arc::clone(&git),
            issues: Arc::clone(&issues),
            issue_bridge: IssueBridgeService::new_with_app(issues, app_handle.clone()),
            files: Arc::new(FileService::new()),
            env: EnvironmentService::new(),
            lsp: LspService::new(),
            app_state_kv: AppStateKvService::new(),
            windows: Arc::new(WindowCoordinatorService::new()),
            tasks: TaskService::new(),
            settings_watchers: SettingsWatcherService::new(),
            file_watchers: FileWatcherService::new(),
            git_watchers: Arc::new(GitWatcherService::new(git)),
            nix_eval_locks: Mutex::new(HashSet::new()),
            resume_discovery: ResumeDiscoveryService::new(),
            activity_map_cache: Mutex::new(None),
            settings_generation: Arc::new(Mutex::new(0)),
        })
    }
}
