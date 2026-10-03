mod common;

use serde_json::json;
use std::{collections::HashSet, fs, sync::Arc, time::Duration};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    Host,
};
use sworm_lib::host_events::DesktopEvent;
use sworm_lib::router::{Target, WorkspaceRouter};
use sworm_protocol::{
    pty::PtyEvent, rpc::AttachMode, session::SessionStartInfo, settings::PatchSettingsSectionInput,
};
use sworm_remote::Identity;
use sworm_server::{serve, ServeOptions};
use tempfile::tempdir;
use tokio::{sync::mpsc, time::timeout};

#[derive(Debug)]
enum PtyDelivery {
    Output(Vec<u8>),
    Event(PtyEvent),
}

fn pty_sinks() -> (
    EventSink<Vec<u8>>,
    EventSink<PtyEvent>,
    mpsc::UnboundedReceiver<PtyDelivery>,
) {
    let (send, receive) = mpsc::unbounded_channel();
    let output_send = send.clone();
    let output = Arc::new(move |bytes| {
        output_send
            .send(PtyDelivery::Output(bytes))
            .map_err(|error| error.to_string())
    });
    let events = Arc::new(move |event| {
        send.send(PtyDelivery::Event(event))
            .map_err(|error| error.to_string())
    });
    (output, events, receive)
}

async fn start_terminal(
    router: &WorkspaceRouter,
    run_id: &str,
    folder: &str,
    owner: &str,
) -> (
    Result<SessionStartInfo, ApiError>,
    mpsc::UnboundedReceiver<PtyDelivery>,
) {
    let (output, events, receive) = pty_sinks();
    let result = router
        .session_start(
            run_id.into(),
            folder.into(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some(owner.into()),
        )
        .await;
    (result, receive)
}

async fn echo(
    router: &WorkspaceRouter,
    run_id: &str,
    receive: &mut mpsc::UnboundedReceiver<PtyDelivery>,
    marker: &str,
) -> Vec<PtyDelivery> {
    router
        .run_write(
            run_id.into(),
            format!("printf '{marker}\\n'\n").into_bytes(),
        )
        .await
        .expect("failed to write PTY marker");
    receive_output_until(receive, format!("{marker}\r\n").as_bytes()).await
}

async fn receive_output_until(
    receive: &mut mpsc::UnboundedReceiver<PtyDelivery>,
    needle: &[u8],
) -> Vec<PtyDelivery> {
    timeout(Duration::from_secs(15), async {
        let mut deliveries = Vec::new();
        let mut output = Vec::new();
        while !output.windows(needle.len()).any(|window| window == needle) {
            let delivery = receive.recv().await.expect("PTY delivery channel closed");
            if let PtyDelivery::Output(bytes) = &delivery {
                output.extend_from_slice(bytes);
            }
            deliveries.push(delivery);
        }
        deliveries
    })
    .await
    .expect("timed out waiting for PTY output")
}
async fn receive_through(
    receive: &mut mpsc::UnboundedReceiver<PtyDelivery>,
    done: fn(&PtyEvent) -> bool,
) -> Vec<PtyDelivery> {
    timeout(Duration::from_secs(15), async {
        let mut deliveries = Vec::new();
        loop {
            let delivery = receive.recv().await.expect("PTY delivery channel closed");
            let finished = matches!(&delivery, PtyDelivery::Event(event) if done(event));
            deliveries.push(delivery);
            if finished {
                return deliveries;
            }
        }
    })
    .await
    .expect("timed out waiting for PTY event")
}

fn output_bytes(deliveries: &[PtyDelivery]) -> Vec<u8> {
    deliveries
        .iter()
        .filter_map(|delivery| match delivery {
            PtyDelivery::Output(bytes) => Some(bytes.as_slice()),
            PtyDelivery::Event(_) => None,
        })
        .flatten()
        .copied()
        .collect()
}

fn count_bytes(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

#[tokio::test(flavor = "multi_thread")]
async fn desktop_remote_router_loopback() -> anyhow::Result<()> {
    // One test owns process-global XDG/HOME for its whole lifetime. Separate
    // integration-test binaries run in separate processes.
    let temporary = tempdir()?;
    common::isolate(temporary.path())?;

    let repository = temporary.path().join("repository");
    common::init_repo(&repository)?;
    fs::create_dir(repository.join("src"))?;
    fs::write(repository.join("src/lib.rs"), "pub fn sentinel() {}\n")?;
    let repository_settings = repository.join(".sworm");
    fs::create_dir(&repository_settings)?;
    fs::write(
        repository_settings.join("settings.jsonc"),
        r#"{"providers":{"terminal":{"enabled":true,"binary_path_override":"sh","extra_args":[]}}}"#,
    )?;

    let (server, desktop_identity, remote_settings) = common::start_loop(temporary.path()).await?;
    let server_address = server.local_addr;
    let server_fingerprint = server.fingerprint;
    let settings_path = temporary.path().join("config-home/sworm/settings.jsonc");

    let (host_events_send, mut host_events_receive) = mpsc::unbounded_channel();
    let host_events: EventSink<DesktopEvent> = Arc::new(move |event| {
        host_events_send
            .send(event)
            .map_err(|error| error.to_string())
    });
    let desktop = Arc::clone(&host_events);
    let host = Arc::new(Host::new(
        temporary.path().join("sworm.db"),
        Arc::new(move |event| desktop(DesktopEvent::Host(event))),
    )?);
    let router = WorkspaceRouter::with_events(Arc::clone(&host), host_events);

    // Pairing has its own daemon: this desktop is already authorized on `loop`.
    let pairing_config = temporary.path().join("pairing-config");
    let pairing_server = serve(ServeOptions {
        config_dir: pairing_config.clone(),
        data_dir: temporary.path().join("pairing-data"),
        listen: Some("127.0.0.1:0".parse()?),
        config_file: None,
        web_assets_dir: None,
    })
    .await?;
    let token = sworm_server::auth::write_pairing_token(&pairing_config)?;
    let pair_link = format!(
        "sworm-pair://{}/{}/{}",
        pairing_server.local_addr, pairing_server.fingerprint, token
    );
    let before_pair = fs::read(&settings_path)?;
    let wrong_link = format!(
        "sworm-pair://{}/{}/wrong-token",
        pairing_server.local_addr, pairing_server.fingerprint
    );
    assert!(router
        .pair_remote(&wrong_link, "paired", false)
        .await
        .is_err());
    assert_eq!(fs::read(&settings_path)?, before_pair);
    let paired = router.pair_remote(&pair_link, "paired", false).await?;
    assert_eq!(paired.fingerprint, pairing_server.fingerprint.to_string());
    let after_pair = fs::read(&settings_path)?;
    assert!(router
        .pair_remote(&pair_link, "paired", false)
        .await
        .is_err());
    assert_eq!(fs::read(&settings_path)?, after_pair);
    let wrong_pin = format!(
        "sworm-pair://{}/SHA256:{}/{}",
        pairing_server.local_addr,
        "00".repeat(32),
        token
    );
    assert!(router
        .pair_remote(&wrong_pin, "paired", true)
        .await
        .is_err());
    assert_eq!(fs::read(&settings_path)?, after_pair);
    let paired_repository = Target::remote_uri("paired", &repository.to_string_lossy());
    router
        .file_read(paired_repository, "hello.txt".into())
        .await?;
    assert!(router.remote_status("paired").await?.connected);
    pairing_server.shutdown().await;
    timeout(Duration::from_secs(10), async {
        loop {
            let status = router.remote_status("paired").await.unwrap();
            if !status.connected {
                assert!(status.last_error.is_some());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    let remote_repository = format!("sworm://loop{}", repository.display());
    let canonical_repository = fs::canonicalize(&repository)?;
    let canonical_uri = Target::remote_uri("loop", &canonical_repository.to_string_lossy());

    let attached = router
        .workbench_attach(
            "lease-window",
            "loop",
            "lease-workbench".into(),
            AttachMode::Open {},
            "lease-window-attachment".into(),
        )
        .await?;
    assert!(matches!(
        attached,
        Some(sworm_protocol::rpc::WorkbenchAttached::Ready { .. })
    ));
    let (started, mut leased_output) =
        start_terminal(&router, "leased-run", &remote_repository, "lease-window").await;
    started?;
    echo(&router, "leased-run", &mut leased_output, "__LEASED_RUN__").await;
    let listed = router.workbench_list("loop").await?;
    let workbench = listed
        .iter()
        .find(|workbench| workbench.id == "lease-workbench")
        .unwrap();
    assert!(workbench.yours);
    assert!(
        workbench.running.iter().any(|run| matches!(
            run,
            sworm_protocol::rpc::WorkbenchRun::Session { provider_id, .. }
                if provider_id == "terminal"
        )),
        "session must belong to workbench, not connection"
    );
    assert!(router
        .workbench_attach(
            "foreign-window",
            "loop",
            "foreign-workbench".into(),
            AttachMode::Open {},
            "foreign-window-attachment".into(),
        )
        .await?
        .is_some());
    let (started, _foreign_deliveries) = start_terminal(
        &router,
        "foreign-release-run",
        &remote_repository,
        "foreign-window",
    )
    .await;
    started?;
    assert!(matches!(
        router.remote_runs_release(
            "lease-window",
            &["foreign-release-run".into(), "leased-run".into()]
        ),
        Err(ApiError::Pty(_))
    ));
    assert!(
        host.pty.run_state("foreign-release-run").is_some(),
        "foreign proxy must stay attached"
    );
    assert!(
        host.pty.run_state("leased-run").is_none(),
        "owned proxy must detach despite foreign error"
    );
    router.session_stop("foreign-release-run".into()).await?;
    router
        .workbench_detach(
            "foreign-window",
            "loop",
            "foreign-workbench".into(),
            "foreign-window-attachment".into(),
        )
        .await?;
    router
        .workbench_close("loop", "foreign-workbench".into())
        .await?;
    router.remote_runs_release(
        "lease-window",
        &[
            "dormant-run".into(),
            "leased-run".into(),
            "leased-run".into(),
        ],
    )?;
    assert!(
        router
            .workbench_list("loop")
            .await?
            .iter()
            .find(|workbench| workbench.id == "lease-workbench")
            .unwrap()
            .running
            .iter()
            .any(|run| matches!(run, sworm_protocol::rpc::WorkbenchRun::Session { .. })),
        "daemon run must remain alive after releasing its desktop proxy"
    );
    router
        .workbench_detach(
            "lease-window",
            "loop",
            "lease-workbench".into(),
            "lease-window-attachment".into(),
        )
        .await?;
    router
        .workbench_close("loop", "lease-workbench".into())
        .await?;

    assert_eq!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await?
            .content,
        "sentinel\n"
    );
    {
        use sha2::{Digest, Sha256};
        let body = vec![b'x'; 20 * 1024 * 1024];
        fs::write(repository.join("large.txt"), &body)?;
        let expected_hash = format!("{:x}", Sha256::digest(&body));
        for (index, folder) in [
            remote_repository.clone(),
            repository.to_string_lossy().into_owned(),
        ]
        .into_iter()
        .enumerate()
        {
            let stat = router.file_stat(folder.clone(), "large.txt".into()).await?;
            let streamed = router
                .read_file_stream(
                    "stream-test",
                    &format!("large-{index}"),
                    folder.clone(),
                    "large.txt".into(),
                    stat.version,
                    stat.size,
                    |_, _| {},
                )
                .await?;
            assert_eq!(streamed.content.as_bytes(), body);
            assert_eq!(streamed.version, expected_hash);
            let stat = router.file_stat(folder.clone(), "large.txt".into()).await?;
            let chunks = std::sync::atomic::AtomicUsize::new(0);
            let interrupted = router
                .read_file_stream(
                    "stream-test",
                    "in-flight",
                    folder,
                    "large.txt".into(),
                    stat.version,
                    stat.size,
                    |bytes, total| {
                        if bytes > 0 {
                            assert!(bytes < total, "cancel while the file is still being read");
                            assert_eq!(
                                chunks.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                                0,
                                "no further chunks should be delivered after cancellation"
                            );
                            router.cancel_file_read("stream-test", "in-flight");
                        }
                    },
                )
                .await;
            assert!(interrupted.is_err());
            assert_eq!(chunks.load(std::sync::atomic::Ordering::Relaxed), 1);
        }
        let huge = fs::File::create(repository.join("huge.txt"))?;
        huge.set_len(300 * 1024 * 1024)?;
        let stat = router
            .file_stat(remote_repository.clone(), "huge.txt".into())
            .await?;
        assert!(router
            .read_file_stream(
                "stream-test",
                "huge",
                remote_repository.clone(),
                "huge.txt".into(),
                stat.version,
                stat.size,
                |_, _| {}
            )
            .await
            .is_err());
        router.cancel_file_read("stream-test", "cancel-first");
        let stat = router
            .file_stat(remote_repository.clone(), "large.txt".into())
            .await?;
        assert!(router
            .read_file_stream(
                "stream-test",
                "cancel-first",
                remote_repository.clone(),
                "large.txt".into(),
                stat.version.clone(),
                stat.size,
                |_, _| {}
            )
            .await
            .is_err());
        let other_owner = router
            .read_file_stream(
                "other-window",
                "cancel-first",
                remote_repository.clone(),
                "large.txt".into(),
                stat.version,
                stat.size,
                |_, _| {},
            )
            .await?;
        assert_eq!(other_owner.version, expected_hash);
        router.release_window("stream-test");
        fs::remove_file(repository.join("large.txt"))?;
        fs::remove_file(repository.join("huge.txt"))?;
    }
    assert!(router
        .files_read_dir(remote_repository.clone(), String::new(), false)
        .await?
        .iter()
        .any(|entry| entry.name == "hello.txt"));
    assert!(
        router
            .git_get_summary(remote_repository.clone())
            .await?
            .is_repo
    );
    assert_eq!(
        router.folder_resolve(remote_repository.clone()).await?.path,
        canonical_uri
    );
    // Browsing a remote folder must hand back remote URIs, or the folder
    // switcher walks out of the workspace on the first click.
    let listed = router
        .folder_list_entries(remote_repository.clone(), false)
        .await?;
    let source = listed
        .iter()
        .find(|entry| entry.name == "src")
        .expect("remote listing includes src");
    assert!(source.is_dir);
    assert_eq!(source.path, format!("{canonical_uri}/src"));
    assert_eq!(
        router
            .file_read(
                repository.to_string_lossy().into_owned(),
                "hello.txt".into(),
            )
            .await?
            .content,
        "sentinel\n"
    );

    router
        .files_watch_dirs(
            "desktop-window".into(),
            remote_repository.clone(),
            vec![String::new()],
        )
        .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    fs::write(repository.join("watched.txt"), "changed\n")?;
    let watched = timeout(Duration::from_secs(5), async {
        loop {
            if let DesktopEvent::Host(HostEvent::FilesChanged(event)) = host_events_receive
                .recv()
                .await
                .expect("host event channel closed")
            {
                if event.dirs.iter().any(String::is_empty) {
                    return event;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for remote file event");
    assert_eq!(watched.folder_path, canonical_uri);

    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: json!({
                    "loop": {
                        "address": "not-a-socket",
                        "fingerprint": server_fingerprint.to_string(),
                    }
                }),
            },
        )
        .await?;
    let changed_address = router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("settings generation change must invalidate cached address");
    assert!(changed_address.to_string().contains("cannot resolve"));

    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: json!({
                    "loop": {
                        "address": server_address.to_string(),
                        "fingerprint": "bad",
                    }
                }),
            },
        )
        .await?;
    assert!(matches!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await
            .expect_err("settings generation change must invalidate cached pin"),
        ApiError::InvalidArgument(_)
    ));

    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: json!({}),
            },
        )
        .await?;
    assert!(router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("removed remote must prune cached client")
        .to_string()
        .contains("Unknown remote server"));

    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: remote_settings.clone(),
            },
        )
        .await?;
    assert_eq!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await?
            .content,
        "sentinel\n"
    );

    // A settings-only remote (as Home Manager writes it) against a daemon with a
    // provisioned identity: the desktop is rejected until the server declares its
    // fingerprint, then admitted with no pairing, as with SSH authorized_keys.
    let server_key = temporary.path().join("declared-server.pem");
    let server_fingerprint = Identity::create(&server_key)?.fingerprint();
    let declared_keys = temporary.path().join("declared-keys");
    fs::write(&declared_keys, "")?;
    let declared_config = temporary.path().join("declared-config");
    fs::create_dir_all(&declared_config)?;
    fs::write(
        declared_config.join("server.jsonc"),
        json!({ "identity_file": server_key, "authorized_keys_file": declared_keys }).to_string(),
    )?;
    let declared_server = serve(ServeOptions {
        config_dir: declared_config.clone(),
        data_dir: temporary.path().join("declared-data"),
        listen: Some("127.0.0.1:0".parse()?),
        config_file: None,
        web_assets_dir: None,
    })
    .await?;
    let mut remotes = remote_settings.clone();
    remotes["declared"] = json!({
        "address": declared_server.local_addr.to_string(),
        "fingerprint": server_fingerprint.to_string(),
    });
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: remotes,
            },
        )
        .await?;
    let declared_repository = Target::remote_uri("declared", &repository.to_string_lossy());
    // Rejection may be an RPC error or the daemon closing QUIC; test access, not wording.
    router
        .file_read(declared_repository.clone(), "hello.txt".into())
        .await
        .expect_err("undeclared desktop must be rejected");
    fs::write(
        &declared_keys,
        format!("{} desktop\n", desktop_identity.fingerprint()),
    )?;
    assert_eq!(
        router
            .file_read(declared_repository, "hello.txt".into())
            .await?
            .content,
        "sentinel\n"
    );
    assert!(!declared_config.join("authorized_keys").exists());
    declared_server.shutdown().await;
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: remote_settings.clone(),
            },
        )
        .await?;
    router
        .files_watch_dirs(
            "desktop-window".into(),
            remote_repository.clone(),
            vec![String::new()],
        )
        .await?;
    assert!(matches!(
        Target::parse("sworm://"),
        Err(ApiError::InvalidArgument(_))
    ));

    assert!(matches!(
        start_terminal(
            &router,
            "unleased-run",
            &remote_repository,
            "desktop-window"
        )
        .await
        .0,
        Err(ApiError::InvalidArgument(_))
    ));
    router
        .workbench_attach(
            "desktop-window",
            "loop",
            "desktop-workbench".into(),
            AttachMode::Open {},
            "desktop-window-attachment".into(),
        )
        .await?;
    let (started, mut deliveries) = start_terminal(
        &router,
        "reconnect-run",
        &remote_repository,
        "desktop-window",
    )
    .await;
    started?;
    let before_loss = echo(
        &router,
        "reconnect-run",
        &mut deliveries,
        "__READY_REMOTE__",
    )
    .await;
    assert!(router.close_remote_for_test("loop").await);
    router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await?;
    let reconnect_replay = receive_through(&mut deliveries, |event| {
        matches!(event, PtyEvent::Synced { .. })
    })
    .await;
    let after_loss = echo(
        &router,
        "reconnect-run",
        &mut deliveries,
        "__AFTER_REMOTE__",
    )
    .await;
    let all_output = [
        output_bytes(&before_loss),
        output_bytes(&reconnect_replay),
        output_bytes(&after_loss),
    ]
    .concat();
    assert_eq!(count_bytes(&all_output, b"__READY_REMOTE__\r\n"), 1);
    assert_eq!(count_bytes(&all_output, b"__AFTER_REMOTE__\r\n"), 1);
    assert!(!reconnect_replay
        .iter()
        .chain(&after_loss)
        .any(|delivery| matches!(
            delivery,
            PtyDelivery::Event(PtyEvent::Error { message, .. })
                if message.contains("lost while disconnected")
        )));
    tokio::time::sleep(Duration::from_secs(2)).await;
    fs::write(repository.join("watched-after-loss.txt"), "changed again\n")?;
    let watched_after_loss = timeout(Duration::from_secs(5), async {
        loop {
            if let DesktopEvent::Host(HostEvent::FilesChanged(event)) = host_events_receive
                .recv()
                .await
                .expect("host event channel closed")
            {
                if event.dirs.iter().any(String::is_empty) {
                    return event;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for restored remote file watch");
    assert_eq!(watched_after_loss.folder_path, canonical_uri);
    let original_pid = before_loss
        .iter()
        .find_map(|delivery| match delivery {
            PtyDelivery::Event(PtyEvent::Started { pid, .. }) => *pid,
            _ => None,
        })
        .expect("original remote process started");
    let (wrong_owner, _) = start_terminal(
        &router,
        "reconnect-run",
        &remote_repository,
        "foreign-window",
    )
    .await;
    assert!(matches!(wrong_owner, Err(ApiError::Pty(_))));
    assert!(router.run_status("reconnect-run".into()).await?.live);
    echo(
        &router,
        "reconnect-run",
        &mut deliveries,
        "__OWNER_STILL_LIVE__",
    )
    .await;

    router
        .workbench_attach(
            "desktop-window",
            "loop",
            "desktop-workbench".into(),
            AttachMode::Takeover {},
            "desktop-window-takeover".into(),
        )
        .await?;
    let (reloaded, mut reloaded_deliveries) = start_terminal(
        &router,
        "reconnect-run",
        &remote_repository,
        "desktop-window",
    )
    .await;
    let reloaded = reloaded?;
    assert!(
        reloaded.resumed,
        "view reload must not restart daemon process"
    );
    let replay = receive_through(&mut reloaded_deliveries, |event| {
        matches!(event, PtyEvent::Synced { .. })
    })
    .await;
    let replay_bytes = output_bytes(&replay);
    for marker in [
        b"__READY_REMOTE__\r\n".as_slice(),
        b"__AFTER_REMOTE__\r\n",
        b"__OWNER_STILL_LIVE__\r\n",
    ] {
        assert_eq!(
            count_bytes(&replay_bytes, marker),
            1,
            "reload replay lost or duplicated output"
        );
    }
    let replay_pid = replay
        .iter()
        .find_map(|delivery| match delivery {
            PtyDelivery::Event(PtyEvent::Started { pid, .. }) => *pid,
            _ => None,
        })
        .expect("replay includes original process start");
    assert_eq!(replay_pid, original_pid);
    echo(
        &router,
        "reconnect-run",
        &mut reloaded_deliveries,
        "__AFTER_RELOAD__",
    )
    .await;
    router.session_stop("reconnect-run".into()).await?;

    router
        .workbench_attach(
            "last-window",
            "loop",
            "last-workbench".into(),
            AttachMode::Open {},
            "last-window-attachment".into(),
        )
        .await?;
    let (started, mut last_owner_deliveries) =
        start_terminal(&router, "last-owner-run", &remote_repository, "last-window").await;
    started?;
    echo(
        &router,
        "last-owner-run",
        &mut last_owner_deliveries,
        "__BEFORE_OWNER_DETACH__",
    )
    .await;
    assert!(router.run_status("last-owner-run".into()).await?.live);
    host.detach_owner("last-window", &HashSet::new());
    assert!(
        router.run_status("last-owner-run".into()).await?.live,
        "last-owner detach must leave daemon run live"
    );
    router
        .workbench_detach(
            "last-window",
            "loop",
            "last-workbench".into(),
            "last-window-attachment".into(),
        )
        .await?;
    router
        .workbench_attach(
            "replacement-window",
            "loop",
            "last-workbench".into(),
            AttachMode::Takeover {},
            "replacement-window-attachment".into(),
        )
        .await?;

    let (resumed, mut resumed_owner_deliveries) = start_terminal(
        &router,
        "last-owner-run",
        &remote_repository,
        "replacement-window",
    )
    .await;
    let resumed = resumed?;
    assert!(resumed.resumed, "last-owner close must detach remote run");
    assert!(router.run_status("last-owner-run".into()).await?.live);
    echo(
        &router,
        "last-owner-run",
        &mut resumed_owner_deliveries,
        "__AFTER_OWNER_DETACH__",
    )
    .await;
    router.session_stop("last-owner-run".into()).await?;

    router
        .workbench_attach(
            "secondary-window",
            "loop",
            "secondary-workbench".into(),
            AttachMode::Open {},
            "secondary-window-attachment".into(),
        )
        .await?;
    let (started, mut secondary_owner_deliveries) = start_terminal(
        &router,
        "secondary-owner-run",
        &remote_repository,
        "secondary-window",
    )
    .await;
    started?;
    echo(
        &router,
        "secondary-owner-run",
        &mut secondary_owner_deliveries,
        "__BEFORE_OWNER_RELEASE__",
    )
    .await;
    assert!(router.run_status("secondary-owner-run".into()).await?.live);
    let closing_host = Arc::clone(&host);
    std::thread::spawn(move || closing_host.release_owner("secondary-window", &HashSet::new()))
        .join()
        .expect("window event thread panicked");
    let (stopping_start, _) = start_terminal(
        &router,
        "secondary-owner-run",
        &remote_repository,
        "replacement-window",
    )
    .await;
    match stopping_start {
        Ok(started) => {
            assert!(!started.resumed, "a stopping run must not be re-adopted");
            router.session_stop("secondary-owner-run".into()).await?;
        }
        Err(ApiError::Pty(message)) => assert!(message.contains("is stopping")),
        Err(error) => return Err(error.into()),
    }
    timeout(Duration::from_secs(15), async {
        while router.server_for_run("secondary-owner-run").is_some() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed out waiting for secondary-owner remote stop");

    let (restarted, mut restarted_owner_deliveries) = start_terminal(
        &router,
        "secondary-owner-run",
        &remote_repository,
        "replacement-window",
    )
    .await;
    let restarted = restarted?;
    assert!(
        !restarted.resumed,
        "secondary-owner close must kill remote run"
    );
    assert!(router.run_status("secondary-owner-run".into()).await?.live);
    echo(
        &router,
        "secondary-owner-run",
        &mut restarted_owner_deliveries,
        "__AFTER_OWNER_RELEASE__",
    )
    .await;
    router.session_stop("secondary-owner-run".into()).await?;

    let (started, mut detached_deliveries) = start_terminal(
        &router,
        "detached-run",
        &remote_repository,
        "replacement-window",
    )
    .await;
    started?;
    echo(
        &router,
        "detached-run",
        &mut detached_deliveries,
        "__BEFORE_DETACH__",
    )
    .await;
    router
        .workbench_detach(
            "replacement-window",
            "loop",
            "last-workbench".into(),
            "replacement-window-attachment".into(),
        )
        .await?;
    assert_eq!(
        host.shutdown().0,
        0,
        "remote shutdown must detach, not kill"
    );
    drop(router);
    drop(host);

    let host = Arc::new(Host::new(
        temporary.path().join("sworm-restarted.db"),
        Arc::new(|_| Ok(())),
    )?);
    let router = WorkspaceRouter::new(Arc::clone(&host));
    router
        .workbench_attach(
            "restarted-window",
            "loop",
            "last-workbench".into(),
            AttachMode::Takeover {},
            "restarted-window-attachment".into(),
        )
        .await?;
    let (resumed, mut resumed_deliveries) = start_terminal(
        &router,
        "detached-run",
        &remote_repository,
        "restarted-window",
    )
    .await;
    let resumed = resumed?;
    assert!(resumed.resumed, "daemon run must survive desktop shutdown");
    echo(
        &router,
        "detached-run",
        &mut resumed_deliveries,
        "__AFTER_DETACH__",
    )
    .await;
    router.session_stop("detached-run".into()).await?;

    let (started, mut completed_deliveries) = start_terminal(
        &router,
        "completed-run",
        &remote_repository,
        "restarted-window",
    )
    .await;
    started?;
    router
        .run_write(
            "completed-run".into(),
            b"printf '\\036COMPLETED_TAIL\\037'; exit 7\n".to_vec(),
        )
        .await?;
    let completed = receive_through(&mut completed_deliveries, |event| {
        matches!(event, PtyEvent::Exit { .. })
    })
    .await;
    assert!(output_bytes(&completed)
        .windows(b"\x1eCOMPLETED_TAIL\x1f".len())
        .any(|window| window == b"\x1eCOMPLETED_TAIL\x1f"));
    drop(router);
    drop(host);

    let host = Arc::new(Host::new(
        temporary.path().join("sworm-replayed.db"),
        Arc::new(|_| Ok(())),
    )?);
    let router = WorkspaceRouter::new(Arc::clone(&host));
    let Some(sworm_protocol::rpc::WorkbenchAttached::Ready {
        controller_token: replayed_token,
        ..
    }) = router
        .workbench_attach(
            "replayed-window",
            "loop",
            "last-workbench".into(),
            AttachMode::Takeover {},
            "replayed-window-attachment".into(),
        )
        .await?
    else {
        panic!("replayed workbench attach was not ready")
    };
    let (replayed, mut replayed_deliveries) = start_terminal(
        &router,
        "completed-run",
        &remote_repository,
        "replayed-window",
    )
    .await;
    let replayed = replayed?;
    assert!(replayed.resumed);
    let replayed = receive_through(&mut replayed_deliveries, |event| {
        matches!(event, PtyEvent::Exit { .. })
    })
    .await;
    assert!(output_bytes(&replayed)
        .windows(b"\x1eCOMPLETED_TAIL\x1f".len())
        .any(|window| window == b"\x1eCOMPLETED_TAIL\x1f"));
    let completed_status = router.run_status("completed-run".into()).await?;
    assert!(!completed_status.live);
    assert_eq!(completed_status.exited, Some(Some(7)));
    router.session_stop("completed-run".into()).await?;

    let (started, mut orphan_deliveries) =
        start_terminal(&router, "orphan-run", &remote_repository, "replayed-window").await;
    started?;
    echo(
        &router,
        "orphan-run",
        &mut orphan_deliveries,
        "__BEFORE_ORPHAN__",
    )
    .await;
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: json!({
                    "loop": {
                        "address": "sworm-unreachable.invalid:7420",
                        "fingerprint": server_fingerprint.to_string(),
                    }
                }),
            },
        )
        .await?;
    assert!(
        router.session_stop("orphan-run".into()).await.is_err(),
        "an unreachable daemon must fail the stop"
    );
    assert_eq!(router.pending_stops_for_test().len(), 1);
    drop(router);
    drop(host);
    let host = Arc::new(Host::new(
        temporary.path().join("sworm-replayed.db"),
        Arc::new(|_| Ok(())),
    )?);
    let router = WorkspaceRouter::new(Arc::clone(&host));
    assert_eq!(
        router.pending_stops_for_test().len(),
        1,
        "a pending stop must survive a desktop restart"
    );
    router.retry_pending_stops();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        router.pending_stops_for_test().len(),
        1,
        "retry must wait for the workbench lease"
    );
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: remote_settings.clone(),
            },
        )
        .await?;
    router
        .workbench_attach(
            "replayed-window",
            "loop",
            "last-workbench".into(),
            AttachMode::Resume {
                controller_token: replayed_token,
            },
            "replayed-window-resumed".into(),
        )
        .await?;
    timeout(Duration::from_secs(60), async {
        while !router.pending_stops_for_test().is_empty() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("timed out waiting for the pending remote stop to land");
    let (orphan_restart, _) =
        start_terminal(&router, "orphan-run", &remote_repository, "replayed-window").await;
    let orphan_restart = orphan_restart?;
    assert!(
        !orphan_restart.resumed,
        "the retried stop must have killed the daemon run"
    );
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: json!({
                    "loop": {
                        "address": "sworm-unreachable.invalid:7420",
                        "fingerprint": server_fingerprint.to_string(),
                    }
                }),
            },
        )
        .await?;
    assert!(router.session_stop("orphan-run".into()).await.is_err());
    assert_eq!(router.pending_stops_for_test().len(), 1);
    assert!(router
        .workbench_detach(
            "replayed-window",
            "loop",
            "last-workbench".into(),
            "replayed-window-resumed".into()
        )
        .await
        .is_err());
    assert_eq!(
        router.pending_stops_for_test().len(),
        1,
        "an unacknowledged detach cannot prove the lease or stop was revoked"
    );
    assert_eq!(
        WorkspaceRouter::new(Arc::clone(&host))
            .pending_stops_for_test()
            .len(),
        1
    );
    router
        .settings_patch_global_section(
            None,
            PatchSettingsSectionInput {
                section: "remotes".into(),
                value: remote_settings,
            },
        )
        .await?;
    router
        .workbench_attach(
            "replayed-window",
            "loop",
            "last-workbench".into(),
            AttachMode::Takeover {},
            "replayed-window-takeover".into(),
        )
        .await?;
    assert!(
        router.pending_stops_for_test().is_empty(),
        "a takeover must discard stops authorized by the previous controller token"
    );
    assert!(
        start_terminal(&router, "orphan-run", &remote_repository, "replayed-window")
            .await
            .0?
            .resumed,
        "the run remains visible after its desktop lease is released"
    );
    router.session_stop("orphan-run".into()).await?;

    server.shutdown().await;
    Ok(())
}
