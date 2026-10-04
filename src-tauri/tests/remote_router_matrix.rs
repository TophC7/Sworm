//! Remote spawn failures, workbench leases, file URIs and breadcrumb roots stay
//! on the daemon; desktop settings and recent folders stay local.

mod common;

use serde_json::json;
use std::{fs, future::Future, path::PathBuf, sync::Arc, time::Duration};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    Host,
};
use sworm_lib::host_events::DesktopEvent;
use sworm_lib::router::{Target, WorkspaceRouter};
use sworm_protocol::folder::PathRootKind;
use sworm_protocol::rpc::{AttachMode, RecentFolder, Request};
use sworm_protocol::{lsp::LspEvent, pty::PtyEvent, settings::PatchSettingsSectionInput};
use sworm_remote::{Identity, RemoteClient};
use sworm_server::ServerHandle;
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
    endpoint: quinn::Endpoint,
    identity: Identity,
}

impl Fixture {
    async fn start(root: TempDir) -> anyhow::Result<Self> {
        let (server, identity, _) = common::start_loop(root.path()).await?;

        Ok(Self {
            root,
            server,
            endpoint: quinn::Endpoint::client("0.0.0.0:0".parse()?)?,
            identity,
        })
    }

    async fn client(&self) -> anyhow::Result<RemoteClient> {
        Ok(RemoteClient::connect(
            &self.endpoint,
            self.server.local_addr,
            &self.identity,
            self.server.fingerprint,
        )
        .await?)
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
        common::init_repo(&path)?;

        let (send, events) = mpsc::unbounded_channel();
        let sink: EventSink<DesktopEvent> =
            Arc::new(move |event| send.send(event).map_err(|error| error.to_string()));
        let desktop = Arc::clone(&sink);
        let host = Arc::new(Host::new(
            self.root.path().join(format!("{name}.db")),
            Arc::new(move |event| desktop(DesktopEvent::Host(event))),
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
    events: mpsc::UnboundedReceiver<DesktopEvent>,
}

/// Every call is bounded, so a daemon that stops answering names the operation
/// it died on instead of hanging the gate.
const OP_TIMEOUT: Duration = Duration::from_secs(20);

/// Covers a hang outside an individual RPC, including setup and event waits.
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

/// One runtime owns the daemon and all behaviors that depend on its tasks.
#[tokio::test]
async fn remote_spawns_leases_and_uris_stay_on_daemon() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    common::isolate(root.path())?;
    let fixture = Fixture::start(root).await?;
    let suite = tokio::time::timeout(SUITE_TIMEOUT, async {
        lease_receives_registry_events_without_folder_claims(&fixture).await?;
        local_leases_follow_authority_and_transfer_identity(&fixture).await?;
        independent_servers_do_not_share_transition_gates(&fixture).await?;
        remote_spawn_arms_and_workspace_uris(&fixture).await?;
        remote_file_rename_emits_desktop_file_moved(&fixture).await?;
        remote_paste_stays_on_source_host(&fixture).await?;
        settings_effective_merges_desktop_sections(&fixture).await?;
        local_only_ops_stay_on_desktop(&fixture).await?;
        anyhow::Ok(())
    })
    .await;
    fixture.shutdown().await;
    suite.unwrap_or_else(|_| panic!("the routing suite did not finish within {SUITE_TIMEOUT:?}"))
}

async fn local_leases_follow_authority_and_transfer_identity(
    fixture: &Fixture,
) -> anyhow::Result<()> {
    use sworm_protocol::rpc::WorkbenchAttached;
    let workspace = fixture.workspace("lease-identities")?;
    let router = &workspace.router;
    let id = "lease-identities".to_owned();
    let Some(WorkbenchAttached::Ready { .. }) = router
        .workbench_attach(
            "source",
            "loop",
            id.clone(),
            AttachMode::Open {},
            "old".into(),
        )
        .await?
    else {
        panic!("source attach refused")
    };
    let Some(WorkbenchAttached::Ready {
        controller_token, ..
    }) = router
        .workbench_attach(
            "source",
            "loop",
            id.clone(),
            AttachMode::Takeover {},
            "current".into(),
        )
        .await?
    else {
        panic!("source replacement refused")
    };
    router
        .workbench_detach("source", "loop", id.clone(), "old".into())
        .await?;
    assert!(router.workbench_owned("source", "loop", &id, "current"));
    let snapshot = r#"{"version":4,"activeTabIndex":-1,"tabs":[]}"#;
    router
        .workbench_save("loop", id.clone(), snapshot.into())
        .await?;
    assert!(router
        .workbench_transfer("source", "target", "loop", id.clone(), "old".into())
        .await
        .is_err());
    router
        .session_start(
            "transferred-proxy".into(),
            workspace.remote.clone(),
            "terminal".into(),
            None,
            80,
            24,
            Arc::new(|_| Ok(())),
            Arc::new(|_| Ok(())),
            Some("source".into()),
        )
        .await?;
    router
        .workbench_attach(
            "target",
            "loop",
            "occupied-target".into(),
            AttachMode::Open {},
            "occupied".into(),
        )
        .await?;
    assert!(router
        .workbench_transfer("source", "target", "loop", id.clone(), "current".into())
        .await
        .is_err());
    router
        .workbench_close("loop", "occupied-target".into())
        .await?;
    let transferred = router
        .workbench_transfer("source", "target", "loop", id.clone(), "current".into())
        .await?;
    assert_eq!(
        transferred,
        WorkbenchAttached::Ready {
            attachment_id: "current".into(),
            controller_token: controller_token.clone(),
            snapshot: snapshot.into(),
        }
    );
    router
        .workbench_detach("source", "loop", id.clone(), "current".into())
        .await?;
    assert!(router.workbench_owned("target", "loop", &id, "current"));
    workspace
        .host
        .pty
        .ensure_owner("transferred-proxy", Some("target"))
        .unwrap();
    assert!(router
        .remote_runs_release("source", &["transferred-proxy".into()])
        .is_err());
    assert!(!workspace
        .host
        .pty
        .stop_owned_run("transferred-proxy", "source")
        .unwrap());
    let adopted = router
        .session_start(
            "transferred-proxy".into(),
            workspace.remote.clone(),
            "terminal".into(),
            None,
            80,
            24,
            Arc::new(|_| Ok(())),
            Arc::new(|_| Ok(())),
            Some("target".into()),
        )
        .await?;
    assert!(
        adopted.resumed,
        "local window transfer must retain the daemon process"
    );
    assert!(
        !router
            .workbench_list_for_owner("source", "loop")
            .await?
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .yours
    );
    assert!(
        router
            .workbench_list_for_owner("target", "loop")
            .await?
            .iter()
            .find(|row| row.id == id)
            .unwrap()
            .yours
    );
    router
        .workbench_transfer("target", "source", "loop", id.clone(), "current".into())
        .await?;
    assert!(router.workbench_owned("source", "loop", &id, "current"));
    workspace
        .host
        .pty
        .ensure_owner("transferred-proxy", Some("source"))
        .unwrap();
    router.session_stop("transferred-proxy".into()).await?;

    let outsider = fixture.client().await?;
    outsider
        .call(&Request::WorkbenchAttach {
            id: id.clone(),
            attachment_id: "outsider".into(),
            mode: AttachMode::Takeover {},
            client: "outsider".into(),
        })
        .await?
        .workbench_attach()
        .unwrap();
    // A stale local cache must not redirect the requester to the old window.
    let redirected = router
        .workbench_attach(
            "requester",
            "loop",
            id.clone(),
            AttachMode::Open {},
            "requester".into(),
        )
        .await?;
    assert!(matches!(redirected, Some(WorkbenchAttached::Busy { .. })));
    assert!(!router.workbench_owned("source", "loop", &id, "current"));
    outsider
        .call(&Request::WorkbenchDetach {
            id: id.clone(),
            attachment_id: "outsider".into(),
        })
        .await?
        .workbench_detach()
        .unwrap();
    router.workbench_close("loop", id).await?;
    Ok(())
}

async fn independent_servers_do_not_share_transition_gates(
    fixture: &Fixture,
) -> anyhow::Result<()> {
    let workspace = fixture.workspace("server-gates")?;
    // Bound UDP socket absorbs QUIC traffic without replying, keeping the
    // first server's connect pending rather than depending on DNS timing.
    let stalled = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    workspace.router.settings_patch_global_section(None, PatchSettingsSectionInput {
        section: "remotes".into(),
        value: json!({
            "loop": { "address": format!("localhost:{}", fixture.server.local_addr.port()), "fingerprint": fixture.server.fingerprint.to_string() },
            "stalled": { "address": stalled.local_addr()?.to_string(), "fingerprint": fixture.server.fingerprint.to_string() },
        }),
    }).await?;
    let router = workspace.router.clone();
    let blocked = tokio::spawn(async move {
        router
            .workbench_attach(
                "slow",
                "stalled",
                "slow".into(),
                AttachMode::Open {},
                "slow".into(),
            )
            .await
    });
    let mut packet = [0u8; 2048];
    bounded("waiting_for_stalled_server", stalled.recv_from(&mut packet)).await?;
    let attached = tokio::time::timeout(
        Duration::from_secs(2),
        workspace.router.workbench_attach(
            "fast",
            "loop",
            "fast".into(),
            AttachMode::Open {},
            "fast".into(),
        ),
    )
    .await
    .expect("healthy server waited behind stalled server")?;
    assert!(matches!(
        attached,
        Some(sworm_protocol::rpc::WorkbenchAttached::Ready { .. })
    ));
    assert!(
        !blocked.is_finished(),
        "stalled server must still be pending during healthy attach"
    );
    blocked.abort();
    let _ = blocked.await;
    workspace
        .router
        .workbench_close("loop", "fast".into())
        .await?;
    workspace.router.settings_patch_global_section(None, PatchSettingsSectionInput {
        section: "remotes".into(),
        value: json!({
            "loop": { "address": format!("localhost:{}", fixture.server.local_addr.port()), "fingerprint": fixture.server.fingerprint.to_string() },
        }),
    }).await?;
    Ok(())
}

async fn lease_receives_registry_events_without_folder_claims(
    fixture: &Fixture,
) -> anyhow::Result<()> {
    let mut workspace = fixture.workspace("lease-events")?;
    let id = "lease-events".to_owned();
    workspace
        .router
        .workbench_attach(
            "first-window",
            "loop",
            id.clone(),
            AttachMode::Open {},
            "first-window-attachment".into(),
        )
        .await?;
    let mut received = false;
    for attempt in 0..20 {
        workspace
            .router
            .workbench_attach(
                "first-window",
                "loop",
                id.clone(),
                AttachMode::Takeover {},
                format!("first-window-attachment-{attempt}"),
            )
            .await?;
        if tokio::time::timeout(Duration::from_millis(250), async {
            loop {
                let event = workspace
                    .events
                    .recv()
                    .await
                    .expect("host event sink closed");
                if let DesktopEvent::WorkbenchesChanged { server } = event {
                    assert_eq!(server, "loop");
                    break;
                }
            }
        })
        .await
        .is_ok()
        {
            received = true;
            break;
        }
    }
    assert!(
        received,
        "a workbench lease must open the server event stream without folder claims"
    );
    workspace.router.workbench_close("loop", id).await?;
    Ok(())
}

async fn remote_spawn_arms_and_workspace_uris(fixture: &Fixture) -> anyhow::Result<()> {
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

    let folder_file = bounded(
        "settings_open_folder_file",
        router.settings_open_folder_file(remote.clone()),
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
    let workbench_id = "matrix-workbench".to_owned();
    let attached = bounded(
        "workbench_attach",
        router.workbench_attach(
            "matrix-window",
            "loop",
            workbench_id.clone(),
            AttachMode::Open {},
            "matrix-window-attachment".into(),
        ),
    )
    .await?;
    assert!(matches!(
        attached,
        Some(sworm_protocol::rpc::WorkbenchAttached::Ready { .. })
    ));
    assert!(bounded("workbench_list", router.workbench_list("loop"))
        .await?
        .iter()
        .any(|workbench| workbench.id == workbench_id && workbench.yours));
    bounded(
        "workbench_save",
        router.workbench_save(
            "loop",
            workbench_id.clone(),
            r#"{"version":4,"activeTabIndex":-1,"tabs":[]}"#.into(),
        ),
    )
    .await?;
    let second = bounded(
        "workbench_second_window",
        router.workbench_attach(
            "matrix-second",
            "loop",
            workbench_id.clone(),
            AttachMode::Takeover {},
            "matrix-second-attachment".into(),
        ),
    )
    .await?;
    assert!(
        second.is_none(),
        "opening an occupied workbench must reuse its window"
    );
    bounded(
        "stale_window_detach",
        router.workbench_detach(
            "matrix-second",
            "loop",
            workbench_id.clone(),
            "matrix-second-attachment".into(),
        ),
    )
    .await?;
    assert!(bounded(
        "workbench_list_after_takeover",
        router.workbench_list("loop")
    )
    .await?
    .iter()
    .any(|workbench| workbench.id == workbench_id && workbench.yours));
    bounded(
        "workbench_close",
        router.workbench_close("loop", workbench_id),
    )
    .await?;

    let detached_id = "matrix-detach".to_owned();
    bounded(
        "workbench_attach",
        router.workbench_attach(
            "matrix-window",
            "loop",
            detached_id.clone(),
            AttachMode::Open {},
            "matrix-window-attachment".into(),
        ),
    )
    .await?;
    let replacement_id = "matrix-replacement".to_owned();
    bounded(
        "workbench_replace",
        router.workbench_attach(
            "matrix-window",
            "loop",
            replacement_id.clone(),
            AttachMode::Open {},
            "matrix-window-attachment".into(),
        ),
    )
    .await?;
    bounded(
        "stale_same_window_detach",
        router.workbench_detach(
            "matrix-window",
            "loop",
            detached_id,
            "matrix-window-attachment".into(),
        ),
    )
    .await?;
    assert!(bounded(
        "workbench_list_after_replace",
        router.workbench_list("loop")
    )
    .await?
    .iter()
    .any(|workbench| workbench.id == replacement_id && workbench.yours));
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
            Some("matrix-window".into()),
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
            false,
            discard_bytes,
            discard_pty,
            Some("matrix-window".into()),
        ),
    )
    .await;
    assert!(
        task.is_err(),
        "an unknown task must not spawn a run on the daemon"
    );
    assert_routed("tasks_start", task);
    bounded(
        "workbench_detach",
        router.workbench_detach(
            "matrix-window",
            "loop",
            replacement_id,
            "matrix-window-attachment".into(),
        ),
    )
    .await?;
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
                DesktopEvent::Host(HostEvent::FileMoved {
                    folder_path,
                    old_path,
                    new_path,
                    replace_destination,
                }) => return (folder_path, old_path, new_path, replace_destination),
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
            if let DesktopEvent::Host(HostEvent::FileDeleted(path)) =
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
        workspace.router.settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "terminal".into(),
                value: json!({ "font_size": 17 }),
            },
        ),
    )
    .await?;

    let effective = bounded(
        "settings_get_effective",
        workspace
            .router
            .settings_get_effective(Some(workspace.remote.clone())),
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

/// A second daemon event on a distinct folder brackets the recent mutation in
/// its broadcast stream. Observing it proves the desktop sink has processed
/// the intervening remote event; no sleep-based absence check is needed.
async fn local_only_ops_stay_on_desktop(fixture: &Fixture) -> anyhow::Result<()> {
    let mut workspace = fixture.workspace("local-only-repo")?;
    let router = &workspace.router;
    let client = fixture.client().await?;
    let key = "workbench:desktop-matrix".to_owned();
    let formatted = "{\n  \"tabs\": [ ],\n  \"marker\": \"desktop\"\n}\n".to_owned();
    bounded(
        "app_state_put",
        router.app_state_put(key.clone(), formatted.clone()),
    )
    .await?;
    assert_eq!(
        bounded(
            "app_state_get",
            tokio::task::spawn_blocking({
                let host = Arc::clone(&workspace.host);
                let key = key.clone();
                move || host.app_state_get(key)
            })
        )
        .await??,
        Some(formatted.clone()),
        "router must persist exact opaque bytes to desktop Host DB"
    );
    assert_eq!(
        bounded(
            "daemon app_state_get",
            client.call(&Request::AppStateGet { key: key.clone() }),
        )
        .await?
        .app_state_get()
        .map_err(sworm_remote::RemoteError::Wire)?,
        None,
        "router's local-only KV write must not reach daemon DB"
    );
    bounded("app_state_delete", router.app_state_delete(key.clone())).await?;
    assert_eq!(
        bounded(
            "app_state_get deleted",
            tokio::task::spawn_blocking({
                let host = Arc::clone(&workspace.host);
                move || host.app_state_get(key)
            })
        )
        .await??,
        None
    );

    let local_uri = "sworm://loop/srv/opaque folder/%2F?literal".to_owned();
    let paths = |folders: Vec<RecentFolder>| -> Vec<String> {
        folders.into_iter().map(|folder| folder.path).collect()
    };
    assert_eq!(
        bounded("recent_folders_list", router.recent_folders_list()).await?,
        Vec::<RecentFolder>::new()
    );
    let touched = bounded(
        "recent_folders_touch",
        router.recent_folders_touch(local_uri.clone()),
    )
    .await?;
    assert!(chrono::DateTime::parse_from_rfc3339(&touched[0].opened_at).is_ok());
    assert_eq!(paths(touched.clone()), vec![local_uri.clone()]);
    match bounded("desktop recent event", workspace.events.recv()).await {
        Some(DesktopEvent::Host(HostEvent::RecentFoldersChanged(folders))) => {
            assert_eq!(folders, touched)
        }
        Some(_) => panic!("desktop recent mutation emitted wrong event"),
        None => panic!("desktop event sink closed"),
    }
    assert_eq!(
        bounded(
            "recent_folders_list persisted",
            tokio::task::spawn_blocking({
                let host = Arc::clone(&workspace.host);
                move || host.recent_folders_list()
            })
        )
        .await??,
        touched
    );

    let home = fixture.root.path().join("home");
    let local_path = home.join("local-path-root");
    fs::create_dir(&local_path)?;
    let root = bounded(
        "folder_path_root local",
        router.folder_path_root(local_path.to_string_lossy().into_owned()),
    )
    .await?;
    assert_eq!(root.path, home.to_string_lossy().as_ref());
    let remote_path = Target::remote_uri("loop", &local_path.to_string_lossy());
    let root = bounded(
        "folder_path_root remote",
        router.folder_path_root(remote_path),
    )
    .await?;
    assert_eq!(
        root.path,
        Target::remote_uri("loop", &home.to_string_lossy())
    );
    assert!(matches!(root.kind, PathRootKind::Home));
    assert_eq!(root.label, "Home");
    let error = bounded(
        "omp_resolve_uri remote cwd",
        router.omp_resolve_uri("local://probe.md".into(), Some(workspace.remote.clone())),
    )
    .await
    .expect_err("remote OMP lookup context must be rejected");
    assert!(matches!(error, ApiError::Remote(_)));

    // NixClear synchronously publishes a folder-scoped remote event. Retry
    // readiness while the router's event subscription opens, then use a
    // different folder for the post-mutation marker so no old event can
    // masquerade as the barrier.
    bounded("remote event subscription readiness", async {
        let mut retry = tokio::time::interval(Duration::from_millis(100));
        loop {
            retry.tick().await;
            router.nix_clear(workspace.remote.clone()).await?;
            while let Ok(event) = workspace.events.try_recv() {
                if let DesktopEvent::Host(HostEvent::NixChanged(path)) = event {
                    if path == workspace.remote {
                        return anyhow::Ok(());
                    }
                }
            }
            if let Ok(Some(DesktopEvent::Host(HostEvent::NixChanged(path)))) =
                tokio::time::timeout(Duration::from_millis(100), workspace.events.recv()).await
            {
                if path == workspace.remote {
                    return anyhow::Ok(());
                }
            }
        }
    })
    .await?;
    let remote_recent = "sworm://remote-only/not-a-desktop-path".to_owned();
    assert_eq!(
        bounded(
            "daemon recent_folders_touch",
            client.call(&Request::RecentFoldersTouch {
                path: remote_recent.clone(),
            }),
        )
        .await?
        .recent_folders_touch()
        .map_err(sworm_remote::RemoteError::Wire)?
        .into_iter()
        .map(|folder| folder.path)
        .collect::<Vec<_>>(),
        vec![remote_recent]
    );
    let marker_folder = fixture.root.path().join("second-event-marker");
    fs::create_dir(&marker_folder)?;
    let marker_uri = Target::remote_uri("loop", &marker_folder.to_string_lossy());
    bounded("remote event marker", router.nix_clear(marker_uri.clone())).await?;
    bounded("remote event marker delivery", async {
        loop {
            match workspace
                .events
                .recv()
                .await
                .expect("desktop event sink closed")
            {
                DesktopEvent::Host(HostEvent::RecentFoldersChanged(paths)) => {
                    panic!("daemon recent folders leaked into desktop sink: {paths:?}")
                }
                DesktopEvent::Host(HostEvent::NixChanged(path)) if path == marker_uri => break,
                _ => {}
            }
        }
    })
    .await;
    assert_eq!(
        paths(bounded("desktop recent_folders_list", router.recent_folders_list()).await?),
        vec![local_uri.clone()],
        "daemon's recent list must not overwrite desktop's list"
    );
    assert_eq!(
        bounded(
            "recent_folders_remove",
            router.recent_folders_remove(vec![local_uri])
        )
        .await?,
        Vec::<RecentFolder>::new()
    );
    match bounded("desktop recent removal event", workspace.events.recv()).await {
        Some(DesktopEvent::Host(HostEvent::RecentFoldersChanged(paths))) => {
            assert!(paths.is_empty())
        }
        Some(_) => panic!("desktop recent removal emitted wrong event"),
        None => panic!("desktop event sink closed"),
    }
    Ok(())
}
