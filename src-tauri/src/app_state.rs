use crate::services::{
    credentials::CredentialService,
    db::{self, DatabaseService},
    env::EnvironmentService,
    git::GitService,
    projects::ProjectService,
    providers::ProviderService,
    pty::PtyService,
    sessions::SessionService,
};
use parking_lot::Mutex;
use std::collections::HashSet;

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
    pub credentials: CredentialService,
    pub env: EnvironmentService,
    /// Tracks project IDs with Nix evaluations in progress to prevent concurrent runs.
    pub nix_eval_locks: Mutex<HashSet<String>>,
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
            credentials: CredentialService::new(),
            env: EnvironmentService::new(),
            nix_eval_locks: Mutex::new(HashSet::new()),
        })
    }
}
