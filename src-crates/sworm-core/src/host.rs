use crate::events::{EventSink, HostEvent};
use crate::services::{
    db::DatabaseService, env::EnvironmentService, file_watcher::FileWatcherService,
    files::FileService, git::GitService, git_watcher::GitWatcherService,
    issue_bridge::IssueBridgeService, issues::IssueService, lsp::LspService,
    providers::ProviderService, pty::PtyService, resume_discovery::ResumeDiscoveryService,
    settings_watcher::SettingsWatcherService, tasks::TaskService,
};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sworm_protocol::activity_map::DiscoveredProject;

/// Services and workflows executing on one development host. The embedding
/// application supplies storage location, event delivery and lifetime policy.
pub struct Host {
    pub db: Arc<DatabaseService>,
    pub(crate) providers: ProviderService,
    pub pty: PtyService,
    pub(crate) git: Arc<GitService>,
    pub(crate) issues: Arc<IssueService>,
    pub(crate) issue_bridge: IssueBridgeService,
    pub(crate) files: Arc<FileService>,
    pub(crate) env: EnvironmentService,
    pub(crate) lsp: LspService,
    pub tasks: TaskService,
    pub(crate) settings_watchers: SettingsWatcherService,
    pub file_watchers: FileWatcherService,
    pub(crate) git_watchers: Arc<GitWatcherService>,
    pub(crate) resume_discovery: ResumeDiscoveryService,
    pub(crate) nix_eval_locks: Mutex<HashSet<String>>,
    pub(crate) activity_map_cache: Mutex<Option<Vec<DiscoveredProject>>>,
    pub(crate) settings_generation: Arc<Mutex<u64>>,
    pub(crate) events: EventSink<HostEvent>,
}

impl Host {
    /// Creates services without opening issue-bridge sockets. Async operations
    /// (including bridge startup) run on the caller's Tokio runtime.
    pub fn new(db_path: PathBuf, events: EventSink<HostEvent>) -> Result<Self, anyhow::Error> {
        let db = Arc::new(DatabaseService::new(db_path)?);
        let issues = Arc::new(IssueService::new());
        let git = Arc::new(GitService::new());
        Ok(Self {
            db,
            providers: ProviderService,
            pty: PtyService::new(),
            git: Arc::clone(&git),
            issues: Arc::clone(&issues),
            issue_bridge: IssueBridgeService::new(issues, Arc::clone(&events)),
            files: Arc::new(FileService::new()),
            env: EnvironmentService::new(),
            lsp: LspService::new(),
            tasks: TaskService::new(),
            settings_watchers: SettingsWatcherService::new(),
            file_watchers: FileWatcherService::new(),
            git_watchers: Arc::new(GitWatcherService::new(git)),
            resume_discovery: ResumeDiscoveryService::new(),
            nix_eval_locks: Mutex::new(HashSet::new()),
            activity_map_cache: Mutex::new(None),
            settings_generation: Arc::new(Mutex::new(0)),
            events,
        })
    }

    /// The caller releases ownership before evicting a folder's shared resources.
    pub fn release_folder(&self, folder: &Path) {
        self.settings_watchers.stop(folder);
        self.git_watchers.stop(folder);
        self.issues.evict(folder);
        self.issue_bridge.stop(folder);
        self.files.evict(folder);
        self.tasks.stop(folder);
        self.git.evict(folder);
    }

    /// Release resources tied to a specific window/owner label while preserving
    /// runs protected by in-flight tab transfers.
    pub fn release_owner(&self, owner: &str, protected_pty_runs: &HashSet<String>) {
        self.file_watchers.release_subscriber(owner);
        self.lsp.kill_owner(owner);
        for run_id in self.pty.kill_owner(owner, protected_pty_runs) {
            self.tasks.release_singleton_by_run_id(&run_id);
        }
    }

    /// Release a last window's resources. Local runs are killed; adopted
    /// shutdown-detaching runs are dropped without invoking their kill policy.
    pub fn detach_owner(&self, owner: &str, protected_pty_runs: &HashSet<String>) {
        self.file_watchers.release_subscriber(owner);
        self.lsp.kill_owner(owner);
        for run_id in self.pty.detach_owner(owner, protected_pty_runs) {
            self.tasks.release_singleton_by_run_id(&run_id);
        }
    }

    pub fn settings_generation(&self) -> u64 {
        *self.settings_generation.lock()
    }

    /// Gracefully terminate local PTYs and language servers while detaching
    /// adopted runs whose backend owns their remote lifetime.
    pub fn shutdown(&self) -> (usize, usize) {
        let pty_cleaned = self.pty.kill_all();
        let lsp_cleaned = self.lsp.kill_all();
        (pty_cleaned, lsp_cleaned)
    }
}
