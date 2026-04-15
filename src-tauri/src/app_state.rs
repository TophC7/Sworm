use crate::models::activity_map::DiscoveredProject;
use crate::services::{
    credentials::CredentialService,
    db::{self, DatabaseService},
    env::EnvironmentService,
    files::FileService,
    git::GitService,
    projects::ProjectService,
    providers::ProviderService,
    pty::PtyService,
    sessions::SessionService,
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
    pub db: Mutex<DatabaseService>,
    pub projects: ProjectService,
    pub providers: Mutex<ProviderService>,
    pub sessions: SessionService,
    pub pty: PtyService,
    pub git: GitService,
    pub files: FileService,
    pub credentials: CredentialService,
    pub env: EnvironmentService,
    /// Tracks project IDs with Nix evaluations in progress to prevent concurrent runs.
    pub nix_eval_locks: Mutex<HashSet<String>>,
    /// Per-cwd locks serializing Codex thread binding to avoid cross-binding races.
    /// Wrapped in Arc so bind threads can evict their entry after completing.
    pub codex_bind_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    /// Cached activity map scan results. None = not yet scanned.
    pub activity_map_cache: Mutex<Option<Vec<DiscoveredProject>>>,
}

impl AppState {
    /// Initialize all services. Database migrations run automatically.
    pub fn new(app_handle: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let db_path = db::resolve_db_path(app_handle)?;
        let db_service = DatabaseService::new(db_path)?;

        let sessions = SessionService::new();

        // On startup there are no live PTYs — any "running" session is stale
        // from a crash, SIGKILL, or dev-mode hot-reload.
        let _ = sessions.reset_stale_running(db_service.conn());

        Ok(Self {
            db: Mutex::new(db_service),
            projects: ProjectService::new(),
            providers: Mutex::new(ProviderService::new()),
            sessions,
            pty: PtyService::new(),
            git: GitService::new(),
            files: FileService::new(),
            credentials: CredentialService::new(),
            env: EnvironmentService::new(),
            nix_eval_locks: Mutex::new(HashSet::new()),
            codex_bind_locks: Arc::new(Mutex::new(HashMap::new())),
            activity_map_cache: Mutex::new(None),
        })
    }
}
