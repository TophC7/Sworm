use serde_json::json;
use std::{collections::HashSet, fs, process::Command, sync::Arc, time::Duration};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    Host,
};
use sworm_lib::router::{Target, WorkspaceRouter};
use sworm_protocol::{pty::PtyEvent, settings::PatchSettingsSectionInput};
use sworm_remote::Identity;
use sworm_server::{auth::append_authorized, serve, ServeOptions};
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
async fn receive_through_synced(
    receive: &mut mpsc::UnboundedReceiver<PtyDelivery>,
) -> Vec<PtyDelivery> {
    timeout(Duration::from_secs(15), async {
        let mut deliveries = Vec::new();
        loop {
            let delivery = receive.recv().await.expect("PTY delivery channel closed");
            let synced = matches!(&delivery, PtyDelivery::Event(PtyEvent::Synced { .. }));
            deliveries.push(delivery);
            if synced {
                return deliveries;
            }
        }
    })
    .await
    .expect("timed out waiting for PTY synchronization")
}

async fn receive_through_exit(
    receive: &mut mpsc::UnboundedReceiver<PtyDelivery>,
) -> Vec<PtyDelivery> {
    timeout(Duration::from_secs(15), async {
        let mut deliveries = Vec::new();
        loop {
            let delivery = receive.recv().await.expect("PTY delivery channel closed");
            let exited = matches!(&delivery, PtyDelivery::Event(PtyEvent::Exit { .. }));
            deliveries.push(delivery);
            if exited {
                return deliveries;
            }
        }
    })
    .await
    .expect("timed out waiting for PTY exit")
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
    let xdg_config_home = temporary.path().join("config-home");
    let xdg_data_home = temporary.path().join("data-home");
    let home = temporary.path().join("home");
    let git_config = temporary.path().join("gitconfig");
    fs::create_dir_all(&home)?;
    fs::write(&git_config, "")?;
    std::env::set_var("XDG_CONFIG_HOME", &xdg_config_home);
    std::env::set_var("XDG_DATA_HOME", &xdg_data_home);
    std::env::set_var("HOME", &home);
    std::env::set_var("GIT_CONFIG_GLOBAL", &git_config);
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");

    let repository = temporary.path().join("repository");
    let git = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .arg(&repository)
        .output()?;
    assert!(
        git.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&git.stderr)
    );
    fs::create_dir(repository.join("src"))?;
    fs::write(repository.join("hello.txt"), "sentinel\n")?;
    fs::write(repository.join("src/lib.rs"), "pub fn sentinel() {}\n")?;
    let repository_settings = repository.join(".sworm");
    fs::create_dir(&repository_settings)?;
    fs::write(
        repository_settings.join("settings.jsonc"),
        r#"{"providers":{"terminal":{"enabled":true,"binary_path_override":"sh","extra_args":[]}}}"#,
    )?;

    let server_config = temporary.path().join("server-config");
    let server = serve(ServeOptions {
        config_dir: server_config.clone(),
        data_dir: temporary.path().join("server-data"),
        listen: Some("127.0.0.1:0".parse()?),
    })
    .await?;
    let server_address = server.local_addr;
    let server_fingerprint = server.fingerprint;

    let desktop_config = xdg_config_home.join("sworm");
    fs::create_dir_all(&desktop_config)?;
    let settings_path = desktop_config.join("settings.jsonc");
    let remote_settings = json!({
        "loop": {
            "address": format!("localhost:{}", server_address.port()),
            "fingerprint": server_fingerprint.to_string(),
        }
    });
    fs::write(
        &settings_path,
        serde_json::to_vec(&json!({ "remotes": remote_settings.clone() }))?,
    )?;
    let desktop_identity = Identity::load_or_generate(&desktop_config, "client")?;
    append_authorized(
        &server_config,
        desktop_identity.fingerprint(),
        "router-test",
    )?;

    let (host_events_send, mut host_events_receive) = mpsc::unbounded_channel();
    let host_events: EventSink<HostEvent> = Arc::new(move |event| {
        host_events_send
            .send(event)
            .map_err(|error| error.to_string())
    });
    let host = Arc::new(Host::new(
        temporary.path().join("sworm.db"),
        Arc::clone(&host_events),
    )?);
    let router = WorkspaceRouter::with_events(Arc::clone(&host), host_events);
    let remote_repository = format!("sworm://loop{}", repository.display());
    let canonical_repository = fs::canonicalize(&repository)?;
    let canonical_uri = Target::remote_uri("loop", &canonical_repository.to_string_lossy());

    assert_eq!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await?,
        "sentinel\n"
    );
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
            .await?,
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
            if let HostEvent::FilesChanged(event) = host_events_receive
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

    host.settings_patch_global_section(PatchSettingsSectionInput {
        section: "remotes".into(),
        value: json!({
            "loop": {
                "address": "not-a-socket",
                "fingerprint": server_fingerprint.to_string(),
            }
        }),
    })
    .await?;
    let changed_address = router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("settings generation change must invalidate cached address");
    assert!(changed_address.to_string().contains("cannot resolve"));

    host.settings_patch_global_section(PatchSettingsSectionInput {
        section: "remotes".into(),
        value: json!({
            "loop": {
                "address": server_address.to_string(),
                "fingerprint": "bad",
            }
        }),
    })
    .await?;
    assert!(matches!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await
            .expect_err("settings generation change must invalidate cached pin"),
        ApiError::InvalidArgument(_)
    ));

    host.settings_patch_global_section(PatchSettingsSectionInput {
        section: "remotes".into(),
        value: json!({}),
    })
    .await?;
    assert!(router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("removed remote must prune cached client")
        .to_string()
        .contains("Unknown remote server"));

    host.settings_patch_global_section(PatchSettingsSectionInput {
        section: "remotes".into(),
        value: remote_settings.clone(),
    })
    .await?;
    assert_eq!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await?,
        "sentinel\n"
    );
    router
        .files_watch_dirs(
            "desktop-window".into(),
            remote_repository.clone(),
            vec![String::new()],
        )
        .await?;
    assert!(matches!(
        Target::parse("sworm://"),
        Err(ApiError::InvalidArgument(message)) if message == "Invalid remote path: sworm://"
    ));

    let (output, events, mut deliveries) = pty_sinks();
    router
        .session_start(
            "reconnect-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("desktop-window".into()),
        )
        .await?;
    router
        .session_write(
            "reconnect-run".into(),
            b"printf '__READY_REMOTE__\\n'\n".to_vec(),
        )
        .await?;
    let before_loss = receive_output_until(&mut deliveries, b"__READY_REMOTE__\r\n").await;
    assert!(router.close_remote_for_test("loop").await);
    router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await?;
    let reconnect_replay = receive_through_synced(&mut deliveries).await;
    router
        .session_write(
            "reconnect-run".into(),
            b"printf '__AFTER_REMOTE__\\n'\n".to_vec(),
        )
        .await?;
    let after_loss = receive_output_until(&mut deliveries, b"__AFTER_REMOTE__\r\n").await;
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
            if let HostEvent::FilesChanged(event) = host_events_receive
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
    router.session_stop("reconnect-run".into()).await?;

    let (output, events, mut last_owner_deliveries) = pty_sinks();
    router
        .session_start(
            "last-owner-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("last-window".into()),
        )
        .await?;
    router
        .session_write(
            "last-owner-run".into(),
            b"printf '__BEFORE_OWNER_DETACH__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(&mut last_owner_deliveries, b"__BEFORE_OWNER_DETACH__\r\n").await;
    assert!(router.run_status("last-owner-run".into()).await?.live);
    host.detach_owner("last-window", &HashSet::new());
    assert!(
        router.run_status("last-owner-run".into()).await?.live,
        "last-owner detach must leave daemon run live"
    );

    let (output, events, mut resumed_owner_deliveries) = pty_sinks();
    let resumed = router
        .session_start(
            "last-owner-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("replacement-window".into()),
        )
        .await?;
    assert!(resumed.resumed, "last-owner close must detach remote run");
    assert!(router.run_status("last-owner-run".into()).await?.live);
    router
        .session_write(
            "last-owner-run".into(),
            b"printf '__AFTER_OWNER_DETACH__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(&mut resumed_owner_deliveries, b"__AFTER_OWNER_DETACH__\r\n").await;
    router.session_stop("last-owner-run".into()).await?;

    let (output, events, mut secondary_owner_deliveries) = pty_sinks();
    router
        .session_start(
            "secondary-owner-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("secondary-window".into()),
        )
        .await?;
    router
        .session_write(
            "secondary-owner-run".into(),
            b"printf '__BEFORE_OWNER_RELEASE__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(
        &mut secondary_owner_deliveries,
        b"__BEFORE_OWNER_RELEASE__\r\n",
    )
    .await;
    assert!(router.run_status("secondary-owner-run".into()).await?.live);
    let closing_host = Arc::clone(&host);
    std::thread::spawn(move || closing_host.release_owner("secondary-window", &HashSet::new()))
        .join()
        .expect("window event thread panicked");
    let (output, events, _) = pty_sinks();
    let stopping_start = router
        .session_start(
            "secondary-owner-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("replacement-window".into()),
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

    let (output, events, mut restarted_owner_deliveries) = pty_sinks();
    let restarted = router
        .session_start(
            "secondary-owner-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("replacement-window".into()),
        )
        .await?;
    assert!(
        !restarted.resumed,
        "secondary-owner close must kill remote run"
    );
    assert!(router.run_status("secondary-owner-run".into()).await?.live);
    router
        .session_write(
            "secondary-owner-run".into(),
            b"printf '__AFTER_OWNER_RELEASE__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(
        &mut restarted_owner_deliveries,
        b"__AFTER_OWNER_RELEASE__\r\n",
    )
    .await;
    router.session_stop("secondary-owner-run".into()).await?;

    let (output, events, mut detached_deliveries) = pty_sinks();
    router
        .session_start(
            "detached-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("last-window".into()),
        )
        .await?;
    router
        .session_write(
            "detached-run".into(),
            b"printf '__BEFORE_DETACH__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(&mut detached_deliveries, b"__BEFORE_DETACH__\r\n").await;
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
    let (output, events, mut resumed_deliveries) = pty_sinks();
    let resumed = router
        .session_start(
            "detached-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("restarted-window".into()),
        )
        .await?;
    assert!(resumed.resumed, "daemon run must survive desktop shutdown");
    router
        .session_write(
            "detached-run".into(),
            b"printf '__AFTER_DETACH__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(&mut resumed_deliveries, b"__AFTER_DETACH__\r\n").await;
    router.session_stop("detached-run".into()).await?;

    let (output, events, mut completed_deliveries) = pty_sinks();
    router
        .session_start(
            "completed-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("restarted-window".into()),
        )
        .await?;
    router
        .session_write(
            "completed-run".into(),
            b"printf '\\036COMPLETED_TAIL\\037'; exit 7\n".to_vec(),
        )
        .await?;
    let completed = receive_through_exit(&mut completed_deliveries).await;
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
    let (output, events, mut replayed_deliveries) = pty_sinks();
    let replayed = router
        .session_start(
            "completed-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("replayed-window".into()),
        )
        .await?;
    assert!(replayed.resumed);
    let replayed = receive_through_exit(&mut replayed_deliveries).await;
    assert!(output_bytes(&replayed)
        .windows(b"\x1eCOMPLETED_TAIL\x1f".len())
        .any(|window| window == b"\x1eCOMPLETED_TAIL\x1f"));
    let completed_status = router.run_status("completed-run".into()).await?;
    assert!(!completed_status.live);
    assert_eq!(completed_status.exited, Some(Some(7)));
    router.session_stop("completed-run".into()).await?;

    let (output, events, mut orphan_deliveries) = pty_sinks();
    router
        .session_start(
            "orphan-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("replayed-window".into()),
        )
        .await?;
    router
        .session_write(
            "orphan-run".into(),
            b"printf '__BEFORE_ORPHAN__\\n'\n".to_vec(),
        )
        .await?;
    receive_output_until(&mut orphan_deliveries, b"__BEFORE_ORPHAN__\r\n").await;
    host.settings_patch_global_section(PatchSettingsSectionInput {
        section: "remotes".into(),
        value: json!({
            "loop": {
                "address": "sworm-unreachable.invalid:7420",
                "fingerprint": server_fingerprint.to_string(),
            }
        }),
    })
    .await?;
    assert!(
        router.session_stop("orphan-run".into()).await.is_err(),
        "an unreachable daemon must fail the stop"
    );
    assert_eq!(router.pending_stops_for_test(), 1);
    assert_eq!(
        WorkspaceRouter::new(Arc::clone(&host)).pending_stops_for_test(),
        1,
        "a pending stop must survive a desktop restart"
    );
    host.settings_patch_global_section(PatchSettingsSectionInput {
        section: "remotes".into(),
        value: remote_settings,
    })
    .await?;
    timeout(Duration::from_secs(60), async {
        while router.pending_stops_for_test() > 0 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("timed out waiting for the pending remote stop to land");
    let (output, events, _) = pty_sinks();
    let orphan_restart = router
        .session_start(
            "orphan-run".into(),
            remote_repository.clone(),
            "terminal".into(),
            None,
            80,
            24,
            output,
            events,
            Some("replayed-window".into()),
        )
        .await?;
    assert!(
        !orphan_restart.resumed,
        "the retried stop must have killed the daemon run"
    );
    router.session_stop("orphan-run".into()).await?;

    server.shutdown().await;
    Ok(())
}
