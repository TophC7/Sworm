use anyhow::{bail, Context, Result};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};
use sworm_protocol::{
    files::DirEntry,
    git::GitSummary,
    pty::PtyEvent,
    rpc::{
        HostEventFrame, HostEventWire, Open, PtyCursor, PtyDown, Request, RunStatus, WireError,
        MAX_REMOTE_FILE_BYTES, MAX_REQUEST_FRAME_BYTES, MAX_STREAMS_PER_CONNECTION,
    },
};
use sworm_remote::{
    wire::{read_frame, read_tagged_frame, write_raw_frame, Frame},
    Fingerprint, Identity, RemoteClient, RemoteError,
};
use sworm_server::{auth, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

const SHORT_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_TIMEOUT: Duration = Duration::from_secs(15);

struct Fixture {
    config: TempDir,
    data: TempDir,
    repo: TempDir,
    client_identity: Identity,
    endpoint: quinn::Endpoint,
    handle: ServerHandle,
}

impl Fixture {
    async fn start(server_toml: Option<&str>) -> Result<Self> {
        let config = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        let repo = tempfile::tempdir()?;
        let client_dir = tempfile::tempdir()?;
        if let Some(contents) = server_toml {
            fs::write(config.path().join("server.toml"), contents)?;
        }
        init_repo(repo.path())?;
        let client_identity = Identity::load_or_generate(client_dir.path(), "client")?;
        let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        let handle = serve(ServeOptions {
            config_dir: config.path().to_path_buf(),
            data_dir: data.path().to_path_buf(),
            listen: Some("127.0.0.1:0".parse()?),
        })
        .await?;
        Ok(Self {
            config,
            data,
            repo,
            client_identity,
            endpoint,
            handle,
        })
    }

    async fn client(&self) -> Result<RemoteClient, RemoteError> {
        RemoteClient::connect(
            &self.endpoint,
            self.handle.local_addr,
            &self.client_identity,
            self.handle.fingerprint,
        )
        .await
    }

    async fn paired_client(&self) -> Result<RemoteClient> {
        let token = auth::write_pairing_token(self.config.path())?;
        let client = self.client().await?;
        client.pair(&token, "test client").await?;
        Ok(client)
    }

    /// Restart the daemon on the same config and data directories, as a
    /// service restart or host reboot would.
    async fn restart(&mut self) -> Result<()> {
        let handle = serve(ServeOptions {
            config_dir: self.config.path().to_path_buf(),
            data_dir: self.data.path().to_path_buf(),
            listen: Some("127.0.0.1:0".parse()?),
        })
        .await?;
        let previous = std::mem::replace(&mut self.handle, handle);
        previous.shutdown().await;
        Ok(())
    }

    fn repo_path(&self) -> String {
        self.repo.path().to_string_lossy().into_owned()
    }
}

fn init_repo(path: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(path)
        .status()
        .context("launch git init")?;
    if !status.success() {
        bail!("git init failed with {status}");
    }
    fs::write(path.join("hello.txt"), "sentinel\n")?;
    fs::create_dir(path.join("src"))?;
    fs::write(path.join("src/lib.rs"), "pub fn sentinel() {}\n")?;
    let sworm_dir = path.join(".sworm");
    fs::create_dir(&sworm_dir)?;
    // Harness configuration is not part of the working-tree assertions.
    fs::write(path.join(".git/info/exclude"), ".sworm/\n")?;
    fs::write(
        sworm_dir.join("settings.jsonc"),
        r#"{"providers":{"terminal":{"enabled":true,"binary_path_override":"sh","extra_args":[]}}}"#,
    )?;
    Ok(())
}

fn assert_unauthorized<T>(result: Result<T, RemoteError>) {
    assert!(matches!(
        result,
        Err(RemoteError::Wire(WireError::Unauthorized { .. }))
    ));
}

async fn wait_closed(client: &RemoteClient) {
    timeout(Duration::from_secs(3), client.closed())
        .await
        .expect("server did not close connection");
}

async fn root_entries(
    client: &RemoteClient,
    project_path: &str,
) -> Result<Vec<DirEntry>, RemoteError> {
    client
        .call(&Request::FilesReadDir {
            project_path: project_path.to_owned(),
            dir_path: String::new(),
            show_hidden: false,
        })
        .await?
        .files_read_dir()
        .map_err(RemoteError::Wire)
}

async fn wait_for_src(client: &RemoteClient, project_path: &str, present: bool) -> Result<()> {
    timeout(SHORT_TIMEOUT, async {
        loop {
            let entries = root_entries(client, project_path).await?;
            if entries.iter().any(|entry| entry.name == "src") == present {
                return Ok::<(), RemoteError>(());
            }
            sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .context("settings change was not reflected in directory listing")??;
    Ok(())
}

async fn watch_root(client: &RemoteClient, project_path: &str) -> Result<()> {
    client
        .call(&Request::FilesWatchDirs {
            project_path: project_path.to_owned(),
            dirs: vec![String::new()],
        })
        .await?
        .files_watch_dirs()
        .map_err(RemoteError::Wire)?;
    Ok(())
}

async fn next_host_event(recv: &mut quinn::RecvStream) -> Result<HostEventWire> {
    Ok(read_frame::<HostEventFrame>(recv).await?.0)
}

async fn wait_for_files_changed(recv: &mut quinn::RecvStream, folder_path: &str) -> Result<()> {
    timeout(SHORT_TIMEOUT, async {
        loop {
            if let HostEventWire::FilesChanged(event) = next_host_event(recv).await? {
                if event.folder_path == folder_path && event.dirs.iter().any(String::is_empty) {
                    return Ok::<(), anyhow::Error>(());
                }
            }
        }
    })
    .await
    .with_context(|| format!("no files-changed event for {folder_path}"))??;
    Ok(())
}

async fn wait_for_created_file(
    recv: &mut quinn::RecvStream,
    folder_path: &str,
    path: &Path,
) -> Result<()> {
    timeout(SHORT_TIMEOUT, async {
        loop {
            if let HostEventWire::FilesChanged(event) = next_host_event(recv).await? {
                if event.folder_path == folder_path
                    && event.dirs.iter().any(String::is_empty)
                    && path.is_file()
                {
                    return Ok::<(), anyhow::Error>(());
                }
            }
        }
    })
    .await
    .with_context(|| format!("file was not created: {}", path.display()))??;
    Ok(())
}

async fn run_status(client: &RemoteClient, run_id: &str) -> Result<RunStatus> {
    Ok(client
        .call(&Request::RunStatus {
            run_id: run_id.to_owned(),
        })
        .await?
        .run_status()
        .map_err(RemoteError::Wire)?)
}

async fn start_terminal(client: &RemoteClient, run_id: &str, folder_path: &str) -> Result<bool> {
    Ok(client
        .call(&Request::SessionStart {
            run_id: run_id.to_owned(),
            folder_path: folder_path.to_owned(),
            provider_id: "terminal".to_owned(),
            resume_token: None,
            cols: 80,
            rows: 24,
        })
        .await?
        .session_start()
        .map_err(RemoteError::Wire)?
        .resumed)
}

async fn stop_session(client: &RemoteClient, run_id: &str) -> Result<()> {
    client
        .call(&Request::SessionStop {
            run_id: run_id.to_owned(),
        })
        .await?
        .session_stop()
        .map_err(RemoteError::Wire)?;
    Ok(())
}

#[derive(Debug)]
enum ObservedPty {
    Output(Vec<u8>),
    Gap(u64),
    Event(PtyEvent),
}

async fn next_pty(recv: &mut quinn::RecvStream, cursor: &mut PtyCursor) -> Result<ObservedPty> {
    match read_tagged_frame::<PtyDown>(recv).await? {
        Frame::Raw(body) => {
            if body.len() < 8 {
                bail!("PTY output frame omitted its offset");
            }
            let start_offset = u64::from_be_bytes(body[..8].try_into().unwrap());
            assert_eq!(
                start_offset, cursor.output_offset,
                "PTY output offsets must be contiguous"
            );
            let bytes = body[8..].to_vec();
            cursor.output_offset += bytes.len() as u64;
            Ok(ObservedPty::Output(bytes))
        }
        Frame::Json(PtyDown::Gap { lost_bytes }) => {
            cursor.output_offset += lost_bytes;
            Ok(ObservedPty::Gap(lost_bytes))
        }
        Frame::Json(PtyDown::Event { sequence, event }) => {
            assert!(sequence >= cursor.event_sequence);
            cursor.event_sequence = sequence;
            Ok(ObservedPty::Event(event))
        }
        Frame::Json(PtyDown::Closed { error }) => bail!("PTY stream closed: {error:?}"),
    }
}

fn occurrences(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

async fn read_until_occurrences(
    recv: &mut quinn::RecvStream,
    cursor: &mut PtyCursor,
    needle: &[u8],
    count: usize,
) -> Result<Vec<u8>> {
    timeout(OUTPUT_TIMEOUT, async {
        let mut output = Vec::new();
        loop {
            match next_pty(recv, cursor).await? {
                ObservedPty::Output(bytes) => {
                    output.extend_from_slice(&bytes);
                    if occurrences(&output, needle) >= count {
                        return Ok::<Vec<u8>, anyhow::Error>(output);
                    }
                }
                ObservedPty::Gap(lost) => bail!("unexpected {lost}-byte PTY gap"),
                ObservedPty::Event(PtyEvent::Error { message, .. }) => {
                    bail!("PTY error before marker: {message}")
                }
                ObservedPty::Event(_) => {}
            }
        }
    })
    .await
    .context("PTY marker timed out")?
}

async fn prepare_shell(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    cursor: &mut PtyCursor,
) -> Result<()> {
    const MARKER: &[u8] = b"SWORM-ECHO-OFF-6a314f";
    write_raw_frame(
        send,
        b"stty -echo; printf '%s%s\\n' 'SWORM-ECHO-' 'OFF-6a314f'\n",
    )
    .await?;
    read_until_occurrences(recv, cursor, MARKER, 1).await?;
    Ok(())
}

async fn wait_for_stream_end(recv: &mut quinn::RecvStream) -> Result<()> {
    timeout(SHORT_TIMEOUT, async {
        loop {
            if read_tagged_frame::<PtyDown>(recv).await.is_err() {
                return;
            }
        }
    })
    .await
    .context("replaced PTY stream stayed open")?;
    Ok(())
}
async fn wait_for_exit(
    recv: &mut quinn::RecvStream,
    cursor: &mut PtyCursor,
    run_id: &str,
) -> Result<Option<i32>> {
    timeout(SHORT_TIMEOUT, async {
        loop {
            match next_pty(recv, cursor).await? {
                ObservedPty::Event(PtyEvent::Exit {
                    run_id: exited_run,
                    code,
                }) if exited_run == run_id => return Ok::<Option<i32>, anyhow::Error>(code),
                ObservedPty::Gap(lost) => bail!("unexpected {lost}-byte gap before PTY exit"),
                ObservedPty::Event(PtyEvent::Error { message, .. }) => {
                    bail!("PTY error before exit: {message}")
                }
                ObservedPty::Output(_) | ObservedPty::Event(_) => {}
            }
        }
    })
    .await
    .context("PTY exit event timed out")?
}

#[tokio::test(flavor = "multi_thread")]
async fn pairing_persists_and_dispatches_host_operations() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let project_path = fixture.repo_path();

    let oversized_request = fixture.client().await?;
    let oversized_result = oversized_request
        .call(&Request::FileRead {
            project_path: format!("/{}", "x".repeat(MAX_REQUEST_FRAME_BYTES)),
            file_path: "ignored".to_string(),
        })
        .await;
    assert!(matches!(oversized_result, Err(RemoteError::Transport(_))));
    oversized_request.close();
    let unpaired = fixture.client().await?;
    assert_unauthorized(
        unpaired
            .call(&Request::FilesReadDir {
                project_path: project_path.clone(),
                dir_path: String::new(),
                show_hidden: false,
            })
            .await,
    );
    wait_closed(&unpaired).await;

    let bad_token = fixture.client().await?;
    assert_unauthorized(bad_token.pair("not-the-token", "intruder").await);
    wait_closed(&bad_token).await;

    fs::write(
        fixture.config.path().join("pairing-token"),
        r#"{"token":"expired-token","expires_at":0}"#,
    )?;
    let expired_token = fixture.client().await?;
    assert_unauthorized(expired_token.pair("expired-token", "old client").await);
    wait_closed(&expired_token).await;

    fs::create_dir(fixture.config.path().join("authorized_keys"))?;
    let doomed_token = auth::write_pairing_token(fixture.config.path())?;
    let persistence_failure = fixture.client().await?;
    let result = persistence_failure
        .pair(&doomed_token, "cannot persist")
        .await;
    assert!(matches!(
        result,
        Err(RemoteError::Wire(WireError::Io { message }))
            if message.contains("persist authorized client")
    ));
    assert!(!fixture.config.path().join("pairing-token").exists());
    persistence_failure.close();
    fs::remove_dir(fixture.config.path().join("authorized_keys"))?;

    let token = auth::write_pairing_token(fixture.config.path())?;
    assert_eq!(
        fs::metadata(fixture.config.path().join("pairing-token"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let pairing_client = fixture.client().await?;
    pairing_client.pair(&token, "test\nlaptop").await?;
    assert_eq!(
        fs::read_to_string(fixture.config.path().join("authorized_keys"))?,
        format!("{} test-laptop\n", fixture.client_identity.fingerprint())
    );
    assert_eq!(
        fs::metadata(fixture.config.path().join("authorized_keys"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(!fixture.config.path().join("pairing-token").exists());

    let client = fixture.client().await?;
    let entries = client
        .call(&Request::FilesReadDir {
            project_path: project_path.clone(),
            dir_path: String::new(),
            show_hidden: false,
        })
        .await?
        .files_read_dir()
        .map_err(RemoteError::Wire)?;
    assert!(entries
        .iter()
        .any(|entry| entry.name == "hello.txt" && !entry.is_dir));
    assert!(entries
        .iter()
        .any(|entry| entry.name == "src" && entry.is_dir));

    let contents = client
        .call(&Request::FileRead {
            project_path: project_path.clone(),
            file_path: "hello.txt".to_string(),
        })
        .await?
        .file_read()
        .map_err(RemoteError::Wire)?;
    assert_eq!(contents, "sentinel\n");

    let oversized_path = fixture.repo.path().join("oversized.txt");
    fs::File::create(&oversized_path)?.set_len(MAX_REMOTE_FILE_BYTES as u64 + 1)?;
    let oversized = client
        .call(&Request::FileRead {
            project_path: project_path.clone(),
            file_path: "oversized.txt".to_string(),
        })
        .await;
    assert!(matches!(
        oversized,
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message.contains("exceeds") && message.contains("read limit")
    ));
    fs::remove_file(oversized_path)?;

    let summary: GitSummary = client
        .call(&Request::GitGetSummary {
            path: project_path.clone(),
        })
        .await?
        .git_get_summary()
        .map_err(RemoteError::Wire)?;
    assert!(summary.is_repo);
    assert_eq!(summary.untracked_count, 2);

    let traversal = client
        .call(&Request::FileRead {
            project_path: project_path.clone(),
            file_path: "../outside".to_string(),
        })
        .await;
    assert!(matches!(
        traversal,
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message.contains("Invalid file path")
    ));

    let relative_project = client
        .call(&Request::FileRead {
            project_path: "relative/project".to_string(),
            file_path: "hello.txt".to_string(),
        })
        .await;
    assert!(matches!(
        relative_project,
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message.contains("must be absolute")
    ));

    let missing = client
        .call(&Request::FilesReadDir {
            project_path: fixture
                .config
                .path()
                .join("missing-project")
                .to_string_lossy()
                .into_owned(),
            dir_path: String::new(),
            show_hidden: false,
        })
        .await;
    assert!(matches!(
        missing,
        Err(RemoteError::Wire(error)) if !matches!(error, WireError::Unauthorized { .. })
    ));

    let bad_pin = RemoteClient::connect(
        &fixture.endpoint,
        fixture.handle.local_addr,
        &fixture.client_identity,
        Fingerprint([0; 32]),
    )
    .await;
    assert!(matches!(
        bad_pin,
        Err(RemoteError::Transport(message)) if message.contains("fingerprint mismatch")
    ));

    client.close();
    pairing_client.close();
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn one_use_token_has_one_winner_across_connections() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let token = auth::write_pairing_token(fixture.config.path())?;
    let first = fixture.client().await?;
    let second_dir = tempfile::tempdir()?;
    let second_identity = Identity::load_or_generate(second_dir.path(), "client")?;
    let second = RemoteClient::connect(
        &fixture.endpoint,
        fixture.handle.local_addr,
        &second_identity,
        fixture.handle.fingerprint,
    )
    .await?;

    let (first_result, second_result) =
        tokio::join!(first.pair(&token, "first"), second.pair(&token, "second"));
    assert_eq!(
        usize::from(first_result.is_ok()) + usize::from(second_result.is_ok()),
        1
    );
    let losing_result = if first_result.is_err() {
        first_result
    } else {
        second_result
    };
    assert_unauthorized(losing_result);
    assert_eq!(
        fs::read_to_string(fixture.config.path().join("authorized_keys"))?
            .lines()
            .count(),
        1
    );
    assert!(!fixture.config.path().join("pairing-token").exists());

    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn static_token_pairs_without_pending_token_file() -> Result<()> {
    let fixture = Fixture::start(Some("auth_token = \"static-secret\"\n")).await?;
    let client = fixture.client().await?;
    client.pair("static-secret", "declarative client").await?;
    assert!(!fixture.config.path().join("pairing-token").exists());
    assert_eq!(
        fs::read_to_string(fixture.config.path().join("authorized_keys"))?,
        format!(
            "{} declarative-client\n",
            fixture.client_identity.fingerprint()
        )
    );

    let contents = client
        .call(&Request::FileRead {
            project_path: fixture.repo_path(),
            file_path: "hello.txt".to_string(),
        })
        .await?
        .file_read()
        .map_err(RemoteError::Wire)?;
    assert_eq!(contents, "sentinel\n");

    fixture.handle.shutdown().await;
    wait_closed(&client).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn strict_config_rejects_unknown_fields() -> Result<()> {
    let config = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    fs::write(config.path().join("server.toml"), "unknown = true\n")?;
    let result = serve(ServeOptions {
        config_dir: config.path().to_path_buf(),
        data_dir: data.path().to_path_buf(),
        listen: Some("127.0.0.1:0".parse()?),
    })
    .await;
    assert!(matches!(result, Err(error) if error.to_string().contains("unknown field")));
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_static_token_is_rejected() -> Result<()> {
    let config = tempfile::tempdir()?;
    let data = tempfile::tempdir()?;
    fs::write(config.path().join("server.toml"), "auth_token = \"\"\n")?;
    let result = serve(ServeOptions {
        config_dir: config.path().to_path_buf(),
        data_dir: data.path().to_path_buf(),
        listen: Some("127.0.0.1:0".parse()?),
    })
    .await;
    assert!(
        matches!(result, Err(error) if error.to_string().contains("auth_token must not be empty"))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn fifo_read_is_rejected_and_shutdown_stays_prompt() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let fifo_path = fixture.repo.path().join("pipe");
    let status = Command::new("mkfifo").arg(&fifo_path).status()?;
    if !status.success() {
        bail!("mkfifo failed with {status}");
    }

    let result = timeout(
        SHORT_TIMEOUT,
        client.call(&Request::FileRead {
            project_path: fixture.repo_path(),
            file_path: "pipe".to_string(),
        }),
    )
    .await
    .context("FIFO read did not return promptly")?;
    assert!(matches!(
        result,
        Err(RemoteError::Wire(WireError::InvalidArgument { message }))
            if message.contains("not a regular file")
    ));

    timeout(SHORT_TIMEOUT, fixture.handle.shutdown())
        .await
        .context("server shutdown stalled after FIFO read")?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn folder_settings_changes_refresh_directory_listing() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let project_path = fixture.repo_path();
    let sworm_dir = fixture.repo.path().join(".sworm");
    fs::create_dir_all(&sworm_dir)?;

    let first = fixture.paired_client().await?;
    assert!(root_entries(&first, &project_path)
        .await?
        .iter()
        .any(|entry| entry.name == "src"));
    let second = fixture.client().await?;
    assert!(root_entries(&second, &project_path)
        .await?
        .iter()
        .any(|entry| entry.name == "src"));

    let settings_path = sworm_dir.join("settings.jsonc");
    fs::write(&settings_path, r#"{"explorer":{"exclude":{"src":true}}}"#)?;
    wait_for_src(&first, &project_path, false).await?;
    wait_for_src(&second, &project_path, false).await?;

    first.close();
    sleep(Duration::from_millis(100)).await;
    fs::write(&settings_path, r#"{"explorer":{"exclude":{}}}"#)?;
    wait_for_src(&second, &project_path, true).await?;

    second.close();
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn idle_streams_do_not_starve_paired_requests() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let idle = fixture.client().await?;
    let paired = fixture.paired_client().await?;
    let body = serde_json::to_vec(&Open::Rpc(Request::FileRead {
        project_path: fixture.repo_path(),
        file_path: "hello.txt".to_owned(),
    }))?;

    let mut idle_streams = Vec::new();
    for _ in 0..MAX_STREAMS_PER_CONNECTION {
        let (mut send, recv) = idle.connection().open_bi().await?;
        send.write_all(&(body.len() as u32).to_be_bytes()).await?;
        send.write_all(&[0, body[0]]).await?;
        idle_streams.push((send, recv));
    }
    sleep(Duration::from_millis(100)).await;

    let entries = timeout(SHORT_TIMEOUT, root_entries(&paired, &fixture.repo_path()))
        .await
        .context("idle streams starved a paired request")??;
    assert!(entries.iter().any(|entry| entry.name == "src"));

    drop(idle_streams);
    paired.close();
    idle.close();
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn events_stream_delivers_only_claimed_folder_changes() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let first = fixture.paired_client().await?;
    let second = fixture.client().await?;
    let other = tempfile::tempdir()?;
    let project_path = fixture.repo_path();
    let other_path = other.path().to_string_lossy().into_owned();
    let (_first_send, mut first_events) = first.open_stream(Open::Events).await?;
    let (_second_send, mut second_events) = second.open_stream(Open::Events).await?;

    watch_root(&first, &project_path).await?;
    watch_root(&second, &other_path).await?;
    fs::write(other.path().join("stream-ready"), "ready\n")?;
    wait_for_files_changed(&mut second_events, &other_path).await?;

    fs::write(fixture.repo.path().join("claimed-change"), "changed\n")?;
    wait_for_files_changed(&mut first_events, &project_path).await?;
    let leaked = timeout(Duration::from_millis(750), async {
        loop {
            match next_host_event(&mut second_events).await? {
                HostEventWire::FilesChanged(event) if event.folder_path == project_path => {
                    return Ok::<bool, anyhow::Error>(true)
                }
                _ => {}
            }
        }
    })
    .await;
    assert!(
        leaked.is_err(),
        "unclaimed folder event crossed connections"
    );

    first.close();
    second.close();
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn session_start_status_and_stop_are_idempotent() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let project_path = fixture.repo_path();
    let run_id = "loopback-idempotent-session";

    assert_eq!(
        run_status(&client, "unknown-loopback-run").await?,
        RunStatus {
            live: false,
            exited: None,
        }
    );
    assert!(!start_terminal(&client, run_id, &project_path).await?);
    assert_eq!(
        run_status(&client, run_id).await?,
        RunStatus {
            live: true,
            exited: None,
        }
    );
    assert!(start_terminal(&client, run_id, &project_path).await?);

    let other = tempfile::tempdir()?;
    let conflicting = client
        .call(&Request::SessionStart {
            run_id: run_id.to_owned(),
            folder_path: other.path().to_string_lossy().into_owned(),
            provider_id: "terminal".to_owned(),
            resume_token: None,
            cols: 80,
            rows: 24,
        })
        .await;
    assert!(matches!(
        conflicting,
        Err(RemoteError::Wire(WireError::InvalidArgument { .. }))
    ));

    let (mut send, mut recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    prepare_shell(&mut send, &mut recv, &mut cursor).await?;
    write_raw_frame(&mut send, b"exit 7\n").await?;
    assert_eq!(
        wait_for_exit(&mut recv, &mut cursor, run_id).await?,
        Some(7)
    );
    assert_eq!(
        run_status(&client, run_id).await?,
        RunStatus {
            live: false,
            exited: Some(Some(7)),
        }
    );

    stop_session(&client, run_id).await?;
    stop_session(&client, run_id).await?;
    assert_eq!(
        run_status(&client, run_id).await?,
        RunStatus {
            live: false,
            exited: None,
        }
    );

    client.close();
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn pty_stream_replays_from_cursor_without_killing_run() -> Result<()> {
    const READY: &[u8] = b"REPLAY-READY-2f73a1";
    const AGAIN: &[u8] = b"REPLAY-AGAIN-c6840d";
    let fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let run_id = "loopback-cursor-replay";
    start_terminal(&client, run_id, &fixture.repo_path()).await?;

    let (mut first_send, mut first_recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    prepare_shell(&mut first_send, &mut first_recv, &mut cursor).await?;
    write_raw_frame(
        &mut first_send,
        b"printf '%s%s\\n' 'REPLAY-READY-' '2f73a1'\n",
    )
    .await?;
    read_until_occurrences(&mut first_recv, &mut cursor, READY, 1).await?;
    drop(first_send);
    drop(first_recv);

    assert!(run_status(&client, run_id).await?.live);
    let (mut second_send, mut second_recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor,
        })
        .await?;
    write_raw_frame(
        &mut second_send,
        b"printf '%s%s\\n' 'REPLAY-AGAIN-' 'c6840d'\n",
    )
    .await?;
    let replayed = read_until_occurrences(&mut second_recv, &mut cursor, AGAIN, 1).await?;
    assert!(
        !replayed.windows(READY.len()).any(|window| window == READY),
        "cursor replay repeated output already consumed"
    );

    stop_session(&client, run_id).await?;
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn completed_run_replays_from_disk_after_daemon_restart() -> Result<()> {
    const TAIL: &[u8] = b"TRANSCRIPT-TAIL-4be71c";
    let mut fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let run_id = "loopback-completed-transcript";
    start_terminal(&client, run_id, &fixture.repo_path()).await?;

    let (mut send, mut recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    prepare_shell(&mut send, &mut recv, &mut cursor).await?;
    write_raw_frame(
        &mut send,
        b"printf '%s%s\\n' 'TRANSCRIPT-TAIL-' '4be71c'; exit 7\n",
    )
    .await?;
    read_until_occurrences(&mut recv, &mut cursor, TAIL, 1).await?;
    assert_eq!(
        wait_for_exit(&mut recv, &mut cursor, run_id).await?,
        Some(7)
    );
    drop(send);
    drop(recv);

    fixture.restart().await?;
    let client = fixture.client().await?;
    let status = run_status(&client, run_id).await?;
    assert!(!status.live);
    assert_eq!(
        status.exited,
        Some(Some(7)),
        "a restarted daemon must still know how the run ended"
    );

    let (_send, mut recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    read_until_occurrences(&mut recv, &mut cursor, TAIL, 1).await?;
    assert_eq!(
        wait_for_exit(&mut recv, &mut cursor, run_id).await?,
        Some(7)
    );

    stop_session(&client, run_id).await?;
    let (_send, mut recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    assert!(
        matches!(
            read_tagged_frame::<PtyDown>(&mut recv).await?,
            Frame::Json(PtyDown::Closed { .. })
        ),
        "stopping a finished run must drop its transcript"
    );

    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn pty_stream_reopen_replaces_previous_stream() -> Result<()> {
    const LIVE: &[u8] = b"REPLACEMENT-LIVE-d17b39";
    let fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let run_id = "loopback-stream-replacement";
    start_terminal(&client, run_id, &fixture.repo_path()).await?;

    let (mut first_send, mut first_recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    prepare_shell(&mut first_send, &mut first_recv, &mut cursor).await?;
    let (mut replacement_send, mut replacement_recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor,
        })
        .await?;

    wait_for_stream_end(&mut first_recv).await?;
    write_raw_frame(
        &mut replacement_send,
        b"printf '%s%s\\n' 'REPLACEMENT-LIVE-' 'd17b39'\n",
    )
    .await?;
    read_until_occurrences(&mut replacement_recv, &mut cursor, LIVE, 1).await?;
    assert!(run_status(&client, run_id).await?.live);

    stop_session(&client, run_id).await?;
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn pty_stream_survives_quic_connection_drop() -> Result<()> {
    const BEFORE: &[u8] = b"DROP-BEFORE-f594c8";
    const AFTER: &[u8] = b"DROP-AFTER-0b7e26";
    let fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let run_id = "loopback-connection-drop";
    let trigger = fixture.repo.path().join("drop-trigger");
    let status = Command::new("mkfifo").arg(&trigger).status()?;
    if !status.success() {
        bail!("mkfifo failed with {status}");
    }
    start_terminal(&client, run_id, &fixture.repo_path()).await?;

    let (mut send, mut recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    prepare_shell(&mut send, &mut recv, &mut cursor).await?;
    write_raw_frame(
        &mut send,
        b"printf '%s%s\\n' 'DROP-BEFORE-' 'f594c8'; read _ < drop-trigger; printf '%s%s\\n' 'DROP-AFTER-' '0b7e26'\n",
    )
    .await?;
    read_until_occurrences(&mut recv, &mut cursor, BEFORE, 1).await?;

    client.close();
    wait_closed(&client).await;
    drop(send);
    drop(recv);
    let write_result = timeout(
        SHORT_TIMEOUT,
        tokio::task::spawn_blocking(move || fs::write(trigger, "continue\n")),
    )
    .await
    .context("shell did not open connection-drop FIFO")??;
    write_result?;

    let reconnected = fixture.client().await?;
    assert!(run_status(&reconnected, run_id).await?.live);
    let (reconnected_send, mut reconnected_recv) = reconnected
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor,
        })
        .await?;
    let replayed = read_until_occurrences(&mut reconnected_recv, &mut cursor, AFTER, 1).await?;
    assert!(
        !replayed
            .windows(BEFORE.len())
            .any(|window| window == BEFORE),
        "reconnect replay repeated output already consumed"
    );
    drop(reconnected_send);

    stop_session(&reconnected, run_id).await?;
    fixture.handle.shutdown().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_client_recovers_twenty_mebibytes_with_contiguous_offsets() -> Result<()> {
    const OUTPUT_BYTES: u64 = 20 * 1024 * 1024;
    const DONE: &[u8] = b"SLOW-DONE-7f3c9a";
    let fixture = Fixture::start(None).await?;
    let client = fixture.paired_client().await?;
    let run_id = "loopback-slow-reader";
    let project_path = fixture.repo_path();
    let (_events_send, mut events_recv) = client.open_stream(Open::Events).await?;
    watch_root(&client, &project_path).await?;
    start_terminal(&client, run_id, &project_path).await?;

    let (mut send, mut recv) = client
        .open_stream(Open::Pty {
            run_id: run_id.to_owned(),
            cursor: PtyCursor::default(),
        })
        .await?;
    let mut cursor = PtyCursor::default();
    prepare_shell(&mut send, &mut recv, &mut cursor).await?;
    let output_start = cursor.output_offset;
    write_raw_frame(
        &mut send,
        b"yes x | tr -d '\\n' | head -c 20971520; touch slow-produced; printf '%s%s\\n' 'SLOW-DONE-' '7f3c9a'\n",
    )
    .await?;

    wait_for_created_file(
        &mut events_recv,
        &project_path,
        &fixture.repo.path().join("slow-produced"),
    )
    .await?;
    timeout(Duration::from_secs(30), async {
        let mut gaps = 0usize;
        let mut lost = 0u64;
        let mut received = 0u64;
        let mut matched = 0usize;
        loop {
            match next_pty(&mut recv, &mut cursor).await? {
                ObservedPty::Output(bytes) => {
                    received += bytes.len() as u64;
                    for byte in bytes {
                        matched = if byte == DONE[matched] {
                            matched + 1
                        } else if byte == DONE[0] {
                            1
                        } else {
                            0
                        };
                        if matched == DONE.len() {
                            assert!(gaps <= 1, "slow-reader recovery emitted {gaps} gaps");
                            assert!(
                                received + lost >= OUTPUT_BYTES,
                                "output accounting omitted generated bytes: received={received}, lost={lost}, cursor={cursor:?}"
                            );
                            assert!(
                                cursor.output_offset - output_start >= OUTPUT_BYTES,
                                "cursor did not span generated output"
                            );
                            return Ok::<(), anyhow::Error>(());
                        }
                    }
                }
                ObservedPty::Gap(bytes) => {
                    gaps += 1;
                    lost += bytes;
                    matched = 0;
                    assert!(gaps <= 1, "slow-reader recovery emitted multiple gaps");
                }
                ObservedPty::Event(PtyEvent::Error { message, .. }) => {
                    bail!("PTY error during slow-reader recovery: {message}")
                }
                ObservedPty::Event(_) => {}
            }
        }
    })
    .await
    .context("slow reader did not self-heal")??;

    assert!(run_status(&client, run_id).await?.live);
    stop_session(&client, run_id).await?;
    fixture.handle.shutdown().await;
    Ok(())
}
