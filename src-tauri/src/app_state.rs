use crate::models::activity_map::DiscoveredProject;
use crate::services::{
    app_state_kv::AppStateKvService,
    credentials::CredentialService,
    db::{self, DatabaseService},
    env::EnvironmentService,
    files::FileService,
    git::GitService,
    issue_bridge::IssueBridgeService,
    issues::IssueService,
    lsp::LspService,
    providers::ProviderService,
    pty::PtyService,
    settings_watcher::SettingsWatcherService,
    tasks::TaskService,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
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
    pub providers: Mutex<ProviderService>,
    pub pty: PtyService,
    pub git: GitService,
    pub issues: Arc<IssueService>,
    pub issue_bridge: IssueBridgeService,
    pub files: FileService,
    pub credentials: CredentialService,
    pub env: EnvironmentService,
    pub lsp: LspService,
    pub app_state_kv: AppStateKvService,
    pub tasks: TaskService,
    pub settings_watchers: SettingsWatcherService,
    /// Tracks folder paths with Nix evaluations in progress to prevent concurrent runs.
    pub nix_eval_locks: Mutex<HashSet<String>>,
    /// Per-cwd locks serializing resume-token binding (Codex threads,
    /// Antigravity conversations) to avoid cross-binding races.
    /// Wrapped in Arc so bind threads can evict their entry after completing.
    pub bind_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Conversation ids already bound to a session, so two tabs in one
    /// folder can't claim the same Antigravity conversation.
    pub bound_conversation_ids: Arc<Mutex<HashSet<String>>>,
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

        Ok(Self {
            db: db_service,
            providers: Mutex::new(ProviderService::new()),
            pty: PtyService::new(),
            git: GitService::new(),
            issues: Arc::clone(&issues),
            issue_bridge: IssueBridgeService::new(issues),
            files: FileService::new(),
            credentials: CredentialService::new(),
            env: EnvironmentService::new(),
            lsp: LspService::new(),
            app_state_kv: AppStateKvService::new(),
            tasks: TaskService::new(),
            settings_watchers: SettingsWatcherService::new(),
            nix_eval_locks: Mutex::new(HashSet::new()),
            bind_locks: Arc::new(Mutex::new(HashMap::new())),
            bound_conversation_ids: Arc::new(Mutex::new(HashSet::new())),
            activity_map_cache: Mutex::new(None),
            settings_generation: Arc::new(Mutex::new(0)),
        })
    }
}
