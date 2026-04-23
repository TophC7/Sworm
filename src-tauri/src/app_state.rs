use crate::models::activity_map::DiscoveredProject;
use crate::services::{
    credentials::CredentialService,
    db::{self, DatabaseService},
    env::EnvironmentService,
    files::FileService,
    git::GitService,
    lsp::LspService,
    projects::ProjectService,
    providers::ProviderService,
    pty::{PtyService, TranscriptBatcher},
    sessions::SessionService,
    tasks::TaskService,
    transcript::TranscriptService,
    workspace_state::{AppStateKvService, WorkspaceStateService},
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Flush threshold: coalesce PTY output chunks until either ~16 KiB have
/// accumulated or ~500 ms have elapsed. Keeps DB write amplification low
/// without leaving too much unflushed data on the table during a crash.
const TRANSCRIPT_FLUSH_BYTES: usize = 16 * 1024;
const TRANSCRIPT_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

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
    pub lsp: LspService,
    pub transcript: Arc<TranscriptService>,
    pub transcript_batcher: Arc<TranscriptBatcher>,
    pub workspace_state: WorkspaceStateService,
    pub app_state_kv: AppStateKvService,
    pub tasks: TaskService,
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
        let db_service = DatabaseService::new(db_path.clone())?;

        let sessions = SessionService::new();

        // On startup there are no live PTYs — any "running" session is stale
        // from a crash, SIGKILL, or dev-mode hot-reload.
        let _ = sessions.reset_stale_running(db_service.conn());

        let pty = PtyService::new();

        // Transcript persistence runs on its own thread with its own DB
        // connection, so the PTY reader never blocks on sqlite.
        let transcript = Arc::new(TranscriptService::start(db_path));
        let transcript_for_batcher = Arc::clone(&transcript);
        let transcript_batcher = Arc::new(TranscriptBatcher::new(
            TRANSCRIPT_FLUSH_BYTES,
            TRANSCRIPT_FLUSH_INTERVAL,
            move |session_id, bytes| {
                transcript_for_batcher.append(session_id, bytes);
            },
        ));

        // Bridge: every PTY chunk pushes into the batcher, which coalesces
        // and hands coarse batches to the transcript service.
        let batcher_for_sink = Arc::clone(&transcript_batcher);
        pty.set_transcript_sink(Box::new(move |session_id, bytes| {
            batcher_for_sink.push(session_id, bytes);
        }));

        // When a session respawns under the same id, flush the old
        // runtime's tail and forget its batch buffer so its bytes can't
        // leak into the new runtime's output.
        let batcher_for_respawn = Arc::clone(&transcript_batcher);
        pty.set_respawn_cleanup(Box::new(move |session_id| {
            batcher_for_respawn.flush(session_id);
            batcher_for_respawn.forget(session_id);
        }));

        Ok(Self {
            db: Mutex::new(db_service),
            projects: ProjectService::new(),
            providers: Mutex::new(ProviderService::new()),
            sessions,
            pty,
            git: GitService::new(),
            files: FileService::new(),
            credentials: CredentialService::new(),
            env: EnvironmentService::new(),
            lsp: LspService::new(),
            transcript,
            transcript_batcher,
            workspace_state: WorkspaceStateService::new(),
            app_state_kv: AppStateKvService::new(),
            tasks: TaskService::new(),
            nix_eval_locks: Mutex::new(HashSet::new()),
            codex_bind_locks: Arc::new(Mutex::new(HashMap::new())),
            activity_map_cache: Mutex::new(None),
        })
    }
}
