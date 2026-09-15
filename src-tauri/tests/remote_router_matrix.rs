//! Every workspace operation a user can reach either routes to the host that
//! owns the folder or is refused there. The matrix is generated from the op
//! table, so a new operation is covered the moment it is added.

use serde_json::json;
use std::{
    collections::HashMap, fs, future::Future, path::PathBuf, process::Command, sync::Arc,
    time::Duration,
};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    Host,
};
use sworm_lib::router::{Target, WorkspaceRouter};
use sworm_protocol::{
    file_diff::{DiffSource, GitStatus},
    issues::{
        IssueCommentCreateInput, IssueCommentUpdateInput, IssueCreateInput, IssueDependencyInput,
        IssueEpicCreateInput, IssueEpicUpdateInput, IssueListFilters, IssueReadyFilters,
        IssueSearchFilters, IssueUpdateInput,
    },
    lsp::{LspEvent, SaveLspServerConfigInput},
    pty::PtyEvent,
    settings::{
        EffectiveSettingsInput, FolderSettingsFileInput, FormattingSettings, LspTraceLevel,
        NixSettings, PatchSettingsSectionInput, SaveProviderConfigInput,
    },
};
use sworm_remote::Identity;
use sworm_server::{auth::append_authorized, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;
use tokio::sync::mpsc;

/// Names nothing that exists. A routed operation therefore fails on the daemon
/// instead of mutating a real workspace, and a fall-through to the desktop's
/// own host still shows up as a `sworm://` path in the error.
const PROBE: &str = "sworm-matrix-probe";

/// One daemon and one desktop configuration for this binary, owned by the
/// runtime that also serves the daemon. The server runs as tasks on that
/// runtime: if it outlived the runtime it was started on, every later call
/// would talk to a daemon whose tasks no longer run.
struct Fixture {
    root: TempDir,
    server: ServerHandle,
}

impl Fixture {
    /// Points every global path this process reads at a scratch directory.
    /// Synchronous and called before the runtime has any work: a task reading
    /// `HOME` while it changes would see either value.
    fn isolate() -> anyhow::Result<TempDir> {
        let root = tempfile::tempdir()?;
        let home = root.path().join("home");
        let git_config = root.path().join("gitconfig");
        fs::create_dir_all(&home)?;
        fs::write(&git_config, "")?;
        std::env::set_var("XDG_CONFIG_HOME", root.path().join("config-home"));
        std::env::set_var("XDG_DATA_HOME", root.path().join("data-home"));
        std::env::set_var("HOME", &home);
        std::env::set_var("GIT_CONFIG_GLOBAL", &git_config);
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        Ok(root)
    }

    async fn start(root: TempDir) -> anyhow::Result<Self> {
        let server_config = root.path().join("server-config");
        let server = serve(ServeOptions {
            config_dir: server_config.clone(),
            data_dir: root.path().join("server-data"),
            listen: Some("127.0.0.1:0".parse()?),
        })
        .await?;

        let desktop_config = root.path().join("config-home").join("sworm");
        fs::create_dir_all(&desktop_config)?;
        fs::write(
            desktop_config.join("settings.jsonc"),
            serde_json::to_vec(&json!({
                "remotes": {
                    "loop": {
                        "address": format!("localhost:{}", server.local_addr.port()),
                        "fingerprint": server.fingerprint.to_string(),
                    }
                }
            }))?,
        )?;
        let identity = Identity::load_or_generate(&desktop_config, "client")?;
        append_authorized(&server_config, identity.fingerprint(), "matrix-test")?;

        Ok(Self { root, server })
    }

    /// Stops the daemon while its runtime is still alive, then releases the
    /// scratch directory.
    async fn shutdown(self) {
        self.server.shutdown().await;
    }

    /// A throwaway repository plus the desktop that reaches it over the loop
    /// server. Each test gets its own so mutations cannot collide.
    fn workspace(&self, name: &str) -> anyhow::Result<Workspace> {
        let path = self.root.path().join(name);
        let git = Command::new("git")
            .args(["-c", "init.defaultBranch=main", "init"])
            .arg(&path)
            .output()?;
        assert!(
            git.status.success(),
            "git init failed: {}",
            String::from_utf8_lossy(&git.stderr)
        );
        fs::write(path.join("hello.txt"), "sentinel\n")?;

        let (send, events) = mpsc::unbounded_channel();
        let sink: EventSink<HostEvent> =
            Arc::new(move |event| send.send(event).map_err(|error| error.to_string()));
        let host = Arc::new(Host::new(
            self.root.path().join(format!("{name}.db")),
            Arc::clone(&sink),
        )?);
        let router = WorkspaceRouter::with_events(Arc::clone(&host), sink);
        let remote = format!("sworm://loop{}", path.display());
        Ok(Workspace {
            host,
            router,
            remote,
            path,
            events,
        })
    }
}

struct Workspace {
    host: Arc<Host>,
    router: WorkspaceRouter,
    /// The `sworm://` URI the frontend would hand every command.
    remote: String,
    path: PathBuf,
    events: mpsc::UnboundedReceiver<HostEvent>,
}

/// Every call is bounded, so a daemon that stops answering names the operation
/// it died on instead of hanging the gate.
const OP_TIMEOUT: Duration = Duration::from_secs(20);

/// Covers a hang outside an individual RPC — setup, event waits, the macro's
/// own expansion of a new operation.
const SUITE_TIMEOUT: Duration = Duration::from_secs(180);

async fn bounded<T>(method: &str, operation: impl Future<Output = T>) -> T {
    match tokio::time::timeout(OP_TIMEOUT, operation).await {
        Ok(value) => value,
        Err(_) => panic!("{method} never answered within {OP_TIMEOUT:?}"),
    }
}

/// A routed call may fail — an empty probe path is meant to fail — but it must
/// fail on the host that owns the folder. Three shapes say it did not: a remote
/// URI in the message (the desktop's own `Host` got the path), a local
/// `Folder not found` for the workspace (the same fall-through with the scheme
/// stripped), and the router's own connectivity errors (the call never left
/// this machine).
fn assert_routed<T>(method: &str, result: Result<T, ApiError>) {
    let Err(error) = result else {
        return;
    };
    let message = error.to_string();
    assert!(
        !message.contains("sworm://"),
        "{method} handed a remote URI to a local host: {message}"
    );
    if let ApiError::NotFound(detail) = &error {
        // The daemon owns the workspace and resolves it; only this machine's
        // host can miss it. `PROBE` names a folder that exists on neither, so a
        // miss on that one is a genuine routed failure.
        assert!(
            !detail.starts_with("Folder not found") || detail.contains(PROBE),
            "{method} resolved the workspace on this machine: {message}"
        );
    }
    for unreachable in ["Unknown remote server", "client identity"] {
        assert!(
            !message.contains(unreachable),
            "{method} never reached the daemon: {message}"
        );
    }
    // Connection, transport and identity failures are the only errors the
    // router prefixes with the server name; an answer from the daemon carries
    // the daemon's own message.
    assert!(
        !matches!(&error, ApiError::Remote(detail) if detail.starts_with("loop: ")),
        "{method} could not reach the daemon: {message}"
    );
}

trait Sample {
    fn sample() -> Self;
}

impl Sample for String {
    fn sample() -> Self {
        PROBE.to_owned()
    }
}

impl Sample for bool {
    fn sample() -> Self {
        false
    }
}

impl Sample for usize {
    fn sample() -> Self {
        0
    }
}

impl Sample for u16 {
    fn sample() -> Self {
        0
    }
}

impl Sample for i64 {
    fn sample() -> Self {
        0
    }
}

impl<T> Sample for Option<T> {
    fn sample() -> Self {
        None
    }
}

impl<T> Sample for Vec<T> {
    fn sample() -> Self {
        Vec::new()
    }
}

impl<K, V> Sample for HashMap<K, V> {
    fn sample() -> Self {
        HashMap::new()
    }
}

impl Sample for DiffSource {
    fn sample() -> Self {
        Self::Working { staged: None }
    }
}

impl Sample for GitStatus {
    fn sample() -> Self {
        Self::Modified
    }
}

impl Sample for IssueListFilters {
    fn sample() -> Self {
        Self::default()
    }
}

impl Sample for IssueReadyFilters {
    fn sample() -> Self {
        Self::default()
    }
}

impl Sample for IssueSearchFilters {
    fn sample() -> Self {
        Self::default()
    }
}

impl Sample for IssueUpdateInput {
    fn sample() -> Self {
        Self::default()
    }
}

impl Sample for IssueEpicUpdateInput {
    fn sample() -> Self {
        Self::default()
    }
}

impl Sample for IssueCreateInput {
    fn sample() -> Self {
        Self {
            title: PROBE.to_owned(),
            description: None,
            status: None,
            priority: None,
            epic_id: None,
            parent_issue_id: None,
            assignee_kind: None,
            assignee_id: None,
            tags: Vec::new(),
            context_json: None,
            actor: None,
        }
    }
}

impl Sample for IssueEpicCreateInput {
    fn sample() -> Self {
        Self {
            title: PROBE.to_owned(),
            description: None,
            status: None,
            priority: None,
            actor: None,
        }
    }
}

impl Sample for IssueCommentCreateInput {
    fn sample() -> Self {
        Self {
            issue_id: PROBE.to_owned(),
            author: PROBE.to_owned(),
            body: PROBE.to_owned(),
            actor: None,
        }
    }
}

impl Sample for IssueCommentUpdateInput {
    fn sample() -> Self {
        Self {
            body: PROBE.to_owned(),
            actor: None,
        }
    }
}

impl Sample for IssueDependencyInput {
    fn sample() -> Self {
        Self {
            issue_id: PROBE.to_owned(),
            depends_on_issue_id: PROBE.to_owned(),
            actor: None,
        }
    }
}

/// The daemon serves from tasks on this runtime, so the runtime must outlive
/// every behavior. A `#[tokio::test]` per behavior gave each one its own
/// runtime: the first to finish dropped the daemon's tasks with it and the rest
/// of the suite talked to a dead server. The behaviors are therefore helpers
/// driven from one runtime here.
#[tokio::test(flavor = "multi_thread")]
async fn remote_workspaces_route_every_reachable_operation() -> anyhow::Result<()> {
    // Process-global, and read by the daemon and desktop alike: set before the
    // runtime has anything to run.
    let root = Fixture::isolate()?;
    let fixture = Fixture::start(root).await?;

    let suite = tokio::time::timeout(SUITE_TIMEOUT, async {
        every_routed_workspace_op_reaches_the_daemon(&fixture).await?;
        remote_file_rename_emits_desktop_file_moved(&fixture).await?;
        remote_paste_stays_on_source_host(&fixture).await?;
        settings_effective_merges_desktop_sections(&fixture).await?;
        anyhow::Ok(())
    })
    .await;

    fixture.shutdown().await;
    suite.unwrap_or_else(|_| panic!("the routing suite did not finish within {SUITE_TIMEOUT:?}"))
}

async fn every_routed_workspace_op_reaches_the_daemon(fixture: &Fixture) -> anyhow::Result<()> {
    let workspace = fixture.workspace("matrix-repo")?;
    let router = &workspace.router;
    let remote = workspace.remote.clone();
    // Prove the pairing and the daemon before reading anything into failures
    // of individual operations below.
    assert_eq!(
        bounded(
            "file_read",
            router.file_read(remote.clone(), "hello.txt".to_owned())
        )
        .await?
        .content,
        "sentinel\n"
    );

    macro_rules! probe_operations {
        // Event sinks, watcher predicates and owners put these outside the
        // generated shape. Each one is invoked by hand below instead.
        (#[route($route:ident)] FilesWatchDirs => $($rest:tt)*) => {};
        (#[route($route:ident)] GitWatch => $($rest:tt)*) => {};
        (#[route($route:ident)] SessionStart => $($rest:tt)*) => {};
        (#[route($route:ident)] TasksStart => $($rest:tt)*) => {};
        (#[route($route:ident)] LspStart => $($rest:tt)*) => {};
        // Not keyed by a folder path: run ids, session ids, an explicit server,
        // or an input struct. Each is asserted by hand below.
        (#[route(run)] $($rest:tt)*) => {};
        (#[route(lsp)] $($rest:tt)*) => {};
        (#[route(none)] $($rest:tt)*) => {};
        (#[route(server)] $($rest:tt)*) => {};
        (#[route(opt_folder_path)] $($rest:tt)*) => {};
        (#[route(input_folder_path)] $($rest:tt)*) => {};
        (#[route(input_path)] $($rest:tt)*) => {};
        (
            #[route($route:ident)]
            $variant:ident => $method:ident(
                $($argument:ident: $argument_type:ty),* $(,)?
            ) -> $return_type:ty;
        ) => {
            $(
                // The route argument is shadowed below, never read as a sample.
                #[allow(unused_variables)]
                let $argument: $argument_type = Sample::sample();
            )*
            // The route argument is what the frontend passes: a workspace URI.
            let $route = remote.clone();
            assert_routed(
                stringify!($method),
                bounded(stringify!($method), router.$method($($argument),*)).await,
            );
        };
        (
            $(
                #[route($route:ident)]
                $variant:ident => $method:ident(
                    $($argument:ident: $argument_type:ty),* $(,)?
                ) -> $return_type:ty;
            )+
        ) => {
            $(
                {
                    probe_operations! {
                        #[route($route)]
                        $variant => $method(
                            $($argument: $argument_type),*
                        ) -> $return_type;
                    }
                }
            )+
        };
    }

    sworm_protocol::sworm_rpc_ops!(probe_operations);

    // The route keys the generated shape cannot express.
    assert_routed(
        "lsp_list_servers",
        bounded(
            "lsp_list_servers",
            router.lsp_list_servers(Some(remote.clone())),
        )
        .await,
    );
    assert_routed(
        "settings_get_effective",
        bounded(
            "settings_get_effective",
            router.settings_get_effective(EffectiveSettingsInput {
                folder_path: Some(remote.clone()),
            }),
        )
        .await,
    );
    let folder_file = bounded(
        "settings_open_folder_file",
        router.settings_open_folder_file(FolderSettingsFileInput {
            folder_path: remote.clone(),
        }),
    )
    .await?;
    assert!(
        folder_file.path.starts_with("sworm://loop/"),
        "folder settings on a remote workspace must name the daemon's file: {}",
        folder_file.path
    );
    assert!(folder_file.path.ends_with(".sworm/settings.jsonc"));
    assert!(
        fs::read_dir(workspace.path.join(".sworm")).is_ok(),
        "the daemon must have created the folder settings file"
    );
    assert_routed(
        "settings_set_nix",
        bounded(
            "settings_set_nix",
            router.settings_set_nix(Some("loop".to_owned()), NixSettings::default()),
        )
        .await,
    );
    assert_routed(
        "settings_get",
        bounded("settings_get", router.settings_get(Some("loop".to_owned()))).await,
    );

    // Watchers are subscriptions, not probes: they must succeed on the daemon.
    // Releasing the folder afterwards drops the claim, so a reconnect does not
    // re-subscribe watchers nothing is listening to.
    bounded(
        "files_watch_dirs",
        router.files_watch_dirs(
            "matrix".to_owned(),
            remote.clone(),
            // The workspace root, relative to the folder the daemon opened.
            vec![String::new()],
        ),
    )
    .await?;
    bounded("git_watch", router.git_watch(remote.clone(), |_| true)).await?;
    router.release_folder(&remote);

    // Starts name a provider, a task and a language server that exist on no
    // host, so the daemon refuses each before it spawns anything: the routing
    // is proven without leaking a process.
    let discard_bytes: EventSink<Vec<u8>> = Arc::new(|_| Ok(()));
    let discard_pty: EventSink<PtyEvent> = Arc::new(|_| Ok(()));
    let discard_lsp: EventSink<LspEvent> = Arc::new(|_| Ok(()));
    let session = bounded(
        "session_start",
        router.session_start(
            format!("{PROBE}-session"),
            remote.clone(),
            PROBE.to_owned(),
            None,
            80,
            24,
            Arc::clone(&discard_bytes),
            Arc::clone(&discard_pty),
            None,
        ),
    )
    .await;
    assert!(
        session.is_err(),
        "an unknown provider must not spawn a session on the daemon"
    );
    assert_routed("session_start", session);
    let task = bounded(
        "tasks_start",
        router.tasks_start(
            format!("{PROBE}-task"),
            remote.clone(),
            PROBE.to_owned(),
            None,
            80,
            24,
            discard_bytes,
            discard_pty,
            None,
        ),
    )
    .await;
    assert!(
        task.is_err(),
        "an unknown task must not spawn a run on the daemon"
    );
    assert_routed("tasks_start", task);
    let language_server = bounded(
        "lsp_start",
        router.lsp_start(
            None,
            format!("{PROBE}-lsp"),
            remote.clone(),
            PROBE.to_owned(),
            remote.clone(),
            discard_lsp,
        ),
    )
    .await;
    assert!(
        language_server.is_err(),
        "an unknown server definition must not spawn a language server on the daemon"
    );
    assert_routed("lsp_start", language_server);

    // Host-owned settings are keyed by the server that runs them, so the
    // desktop names `loop` instead of passing a path.
    assert_routed(
        "settings_patch_global_section",
        bounded(
            "settings_patch_global_section",
            router.settings_patch_global_section(
                Some("loop".to_owned()),
                PatchSettingsSectionInput {
                    // Refused before anything is written, so the probe cannot
                    // disturb the settings the rest of the suite reads.
                    section: PROBE.to_owned(),
                    value: json!({}),
                },
            ),
        )
        .await,
    );
    assert_routed(
        "settings_set_formatting",
        bounded(
            "settings_set_formatting",
            router.settings_set_formatting(Some("loop".to_owned()), FormattingSettings::default()),
        )
        .await,
    );
    assert_routed(
        "settings_set_provider_config",
        bounded(
            "settings_set_provider_config",
            router.settings_set_provider_config(
                Some("loop".to_owned()),
                SaveProviderConfigInput {
                    provider_id: PROBE.to_owned(),
                    enabled: false,
                    binary_path_override: None,
                    extra_args: Vec::new(),
                },
            ),
        )
        .await,
    );
    assert_routed(
        "lsp_set_server_config",
        bounded(
            "lsp_set_server_config",
            router.lsp_set_server_config(
                Some("loop".to_owned()),
                SaveLspServerConfigInput {
                    server_definition_id: PROBE.to_owned(),
                    enabled: false,
                    binary_path_override: None,
                    runtime_path_override: None,
                    runtime_args: Vec::new(),
                    extra_args: Vec::new(),
                    trace: LspTraceLevel::default(),
                    settings: None,
                },
            ),
        )
        .await,
    );

    Ok(())
}

async fn remote_file_rename_emits_desktop_file_moved(fixture: &Fixture) -> anyhow::Result<()> {
    let mut workspace = fixture.workspace("rename-repo")?;
    // The router echoes the URI it was given, so window claims keep matching.
    let folder = Target::remote_uri("loop", &workspace.path.to_string_lossy());

    bounded(
        "file_rename",
        workspace.router.file_rename(
            workspace.remote.clone(),
            "hello.txt".to_owned(),
            "renamed.txt".to_owned(),
        ),
    )
    .await?;
    assert!(workspace.path.join("renamed.txt").exists());

    let moved = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match workspace.events.recv().await.expect("event channel closed") {
                HostEvent::FileMoved {
                    folder_path,
                    old_path,
                    new_path,
                    replace_destination,
                } => return (folder_path, old_path, new_path, replace_destination),
                _ => continue,
            }
        }
    })
    .await
    .expect("remote rename must emit a desktop FileMoved event");

    assert_eq!(moved.0, PathBuf::from(&folder));
    assert_eq!(moved.1, PathBuf::from(format!("{folder}/hello.txt")));
    assert_eq!(moved.2, PathBuf::from(format!("{folder}/renamed.txt")));
    assert!(!moved.3);

    bounded(
        "file_delete",
        workspace
            .router
            .file_delete(workspace.remote.clone(), "renamed.txt".to_owned()),
    )
    .await?;
    let deleted = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let HostEvent::FileDeleted(path) =
                workspace.events.recv().await.expect("event channel closed")
            {
                return path;
            }
        }
    })
    .await
    .expect("remote delete must emit a desktop FileDeleted event");
    assert_eq!(deleted, PathBuf::from(format!("{folder}/renamed.txt")));

    Ok(())
}

/// File clipboard references stay on their owning host: same-host copies and
/// moves run entirely on the daemon, while local and cross-host paths never
/// reach it.
async fn remote_paste_stays_on_source_host(fixture: &Fixture) -> anyhow::Result<()> {
    let workspace = fixture.workspace("paste-repo")?;
    let local_source = workspace
        .path
        .join("hello.txt")
        .to_string_lossy()
        .into_owned();
    let remote_source = format!("sworm://loop{local_source}");
    fs::create_dir(workspace.path.join("copies"))?;
    fs::create_dir(workspace.path.join("moved"))?;

    let copied = bounded(
        "file_paste copy",
        workspace.router.file_paste(
            workspace.remote.clone(),
            "copies".to_owned(),
            "copy".to_owned(),
            vec![remote_source.clone()],
            "auto_rename".to_owned(),
            None,
        ),
    )
    .await?;
    assert_eq!(copied[0].source, remote_source);
    assert_eq!(copied[0].destination, "copies/hello.txt");
    assert_eq!(
        fs::read_to_string(workspace.path.join("copies/hello.txt"))?,
        "sentinel\n"
    );

    let moved = bounded(
        "file_paste cut",
        workspace.router.file_paste(
            workspace.remote.clone(),
            "moved".to_owned(),
            "cut".to_owned(),
            vec![remote_source],
            "auto_rename".to_owned(),
            None,
        ),
    )
    .await?;
    assert_eq!(moved[0].destination, "moved/hello.txt");
    assert!(!workspace.path.join("hello.txt").exists());
    assert_eq!(
        fs::read_to_string(workspace.path.join("moved/hello.txt"))?,
        "sentinel\n"
    );

    let refused = bounded(
        "file_paste local source",
        workspace.router.file_paste(
            workspace.remote.clone(),
            String::new(),
            "copy".to_owned(),
            vec![local_source],
            "overwrite".to_owned(),
            None,
        ),
    )
    .await
    .expect_err("a local clipboard source must not reach the daemon");
    assert!(matches!(refused, ApiError::Remote(_)), "{refused:?}");

    let cross_host = bounded(
        "file_paste cross-host source",
        workspace.router.file_paste_collisions(
            workspace.remote.clone(),
            String::new(),
            vec!["sworm://other/srv/repo/hello.txt".to_owned()],
        ),
    )
    .await
    .expect_err("a source from another server must not reach this daemon");
    assert!(matches!(cross_host, ApiError::Remote(_)), "{cross_host:?}");

    let into_local = bounded(
        "file_paste remote source into local workspace",
        workspace.router.file_paste_collisions(
            workspace.path.to_string_lossy().into_owned(),
            String::new(),
            vec!["sworm://loop/srv/repo/hello.txt".to_owned()],
        ),
    )
    .await
    .expect_err("a remote clipboard source must not paste into a local workspace");
    assert!(matches!(into_local, ApiError::Remote(_)), "{into_local:?}");

    Ok(())
}

async fn settings_effective_merges_desktop_sections(fixture: &Fixture) -> anyhow::Result<()> {
    let workspace = fixture.workspace("settings-repo")?;
    fs::create_dir_all(workspace.path.join(".sworm"))?;
    fs::write(
        workspace.path.join(".sworm/settings.jsonc"),
        r#"{"terminal":{"font_size":11},"nix":{"eval_timeout_secs":123}}"#,
    )?;
    bounded(
        "settings_patch_global_section",
        workspace
            .host
            .settings_patch_global_section(PatchSettingsSectionInput {
                section: "terminal".into(),
                value: json!({ "font_size": 17 }),
            }),
    )
    .await?;

    let effective = bounded(
        "settings_get_effective",
        workspace
            .router
            .settings_get_effective(EffectiveSettingsInput {
                folder_path: Some(workspace.remote.clone()),
            }),
    )
    .await?;

    assert_eq!(
        effective.settings.terminal.font_size, 17,
        "terminal is a desktop section: the window the user is in owns it"
    );
    assert_eq!(
        effective.settings.nix.eval_timeout_secs, 123,
        "host sections must resolve on the daemon that runs the folder"
    );
    assert!(effective.settings.remotes.contains_key("loop"));

    Ok(())
}

/// The router is the only way a user-supplied path reaches a `Host`. Commands
/// that still touch `state.host` may only use handles or operations that take
/// no workspace path at all.
#[test]
fn commands_reach_hosts_only_through_the_router() {
    const DESKTOP_ONLY: &[&str] = &[
        // Shared handles, not workspace operations.
        "db",
        "pty",
        "file_watchers",
        // Pathless operations about this machine.
        "activity_map_get",
        "activity_map_refresh",
        "builtins_get_catalog",
        "config_schemas_list",
        "provider_list",
        // Resolves local launch targets; rejects remote cwds itself.
        "omp_resolve_uri",
        // Desktop-owned settings layers and sections.
        "settings_get_global_layer",
        "settings_create_global_file",
        "settings_set_window",
        "settings_set_terminal",
    ];

    let mut offenders = Vec::new();
    for entry in fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src/commands"))
        .expect("commands directory")
    {
        let path = entry.expect("directory entry").path();
        if !matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs")
        ) {
            continue;
        }
        let source = fs::read_to_string(&path).expect("command module");
        let dense: String = source
            .chars()
            .filter(|char| !char.is_whitespace())
            .collect();
        for call in dense.split("state.host.").skip(1) {
            let member: String = call
                .chars()
                .take_while(|char| char.is_alphanumeric() || *char == '_')
                .collect();
            if !DESKTOP_ONLY.contains(&member.as_str()) {
                offenders.push(format!(
                    "{}: state.host.{member}",
                    path.file_name().expect("file name").to_string_lossy()
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these commands bypass the workspace router: {offenders:?}"
    );
}
