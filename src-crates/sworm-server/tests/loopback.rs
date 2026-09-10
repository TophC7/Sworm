use anyhow::{bail, Context, Result};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command, time::Duration};
use sworm_protocol::{
    files::DirEntry,
    git::GitSummary,
    rpc::{Request, WireError, MAX_REMOTE_FILE_BYTES, MAX_REQUEST_FRAME_BYTES},
};
use sworm_remote::{Fingerprint, Identity, RemoteClient, RemoteError};
use sworm_server::{auth, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;
use tokio::time::{sleep, timeout};

struct Fixture {
    config: TempDir,
    _data: TempDir,
    repo: TempDir,
    client_identity: Identity,
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
        let handle = serve(ServeOptions {
            config_dir: config.path().to_path_buf(),
            data_dir: data.path().to_path_buf(),
            listen: Some("127.0.0.1:0".parse()?),
        })
        .await?;
        Ok(Self {
            config,
            _data: data,
            repo,
            client_identity,
            handle,
        })
    }

    async fn client(&self) -> Result<RemoteClient, RemoteError> {
        RemoteClient::connect(
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
    Ok(())
}

fn assert_unauthorized<T>(result: Result<T, RemoteError>) {
    assert!(matches!(
        result,
        Err(RemoteError::Wire(WireError::Unauthorized { .. }))
    ));
}

async fn wait_closed(client: &RemoteClient) {
    timeout(Duration::from_secs(3), async {
        while !client.is_closed() {
            sleep(Duration::from_millis(10)).await;
        }
    })
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
        .await
}

async fn wait_for_src(client: &RemoteClient, project_path: &str, present: bool) -> Result<()> {
    timeout(Duration::from_secs(5), async {
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

#[tokio::test(flavor = "multi_thread")]
async fn pairing_persists_and_dispatches_host_operations() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let project_path = fixture.repo_path();

    let oversized_request = fixture.client().await?;
    let oversized_result = oversized_request
        .call::<String>(&Request::FileRead {
            project_path: format!("/{}", "x".repeat(MAX_REQUEST_FRAME_BYTES)),
            file_path: "ignored".to_string(),
        })
        .await;
    assert!(matches!(oversized_result, Err(RemoteError::Transport(_))));
    oversized_request.close();
    let unpaired = fixture.client().await?;
    assert_unauthorized(
        unpaired
            .call::<Vec<DirEntry>>(&Request::FilesReadDir {
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
    let entries: Vec<DirEntry> = client
        .call(&Request::FilesReadDir {
            project_path: project_path.clone(),
            dir_path: String::new(),
            show_hidden: false,
        })
        .await?;
    assert!(entries
        .iter()
        .any(|entry| entry.name == "hello.txt" && !entry.is_dir));
    assert!(entries
        .iter()
        .any(|entry| entry.name == "src" && entry.is_dir));

    let contents: String = client
        .call(&Request::FileRead {
            project_path: project_path.clone(),
            file_path: "hello.txt".to_string(),
        })
        .await?;
    assert_eq!(contents, "sentinel\n");

    let oversized_path = fixture.repo.path().join("oversized.txt");
    fs::File::create(&oversized_path)?.set_len(MAX_REMOTE_FILE_BYTES as u64 + 1)?;
    let oversized = client
        .call::<String>(&Request::FileRead {
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
        .await?;
    assert!(summary.is_repo);
    assert_eq!(summary.untracked_count, 2);

    let traversal = client
        .call::<String>(&Request::FileRead {
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
        .call::<String>(&Request::FileRead {
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
        .call::<Vec<DirEntry>>(&Request::FilesReadDir {
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

    let contents: String = client
        .call(&Request::FileRead {
            project_path: fixture.repo_path(),
            file_path: "hello.txt".to_string(),
        })
        .await?;
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
        Duration::from_secs(5),
        client.call::<String>(&Request::FileRead {
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

    timeout(Duration::from_secs(5), fixture.handle.shutdown())
        .await
        .context("server shutdown stalled after FIFO read")?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn folder_settings_changes_refresh_directory_listing() -> Result<()> {
    let fixture = Fixture::start(None).await?;
    let project_path = fixture.repo_path();
    let sworm_dir = fixture.repo.path().join(".sworm");
    fs::create_dir(&sworm_dir)?;

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
    let mut idle_clients = Vec::new();
    for _ in 0..4 {
        idle_clients.push(fixture.client().await?);
    }
    let paired = fixture.paired_client().await?;

    let mut idle_streams = Vec::new();
    for client in &idle_clients {
        for _ in 0..16 {
            let (mut send, recv) = client.connection().open_bi().await?;
            send.write_all(&[0]).await?;
            idle_streams.push((send, recv));
        }
    }
    sleep(Duration::from_millis(100)).await;

    let entries = timeout(
        Duration::from_secs(5),
        root_entries(&paired, &fixture.repo_path()),
    )
    .await
    .context("idle streams starved a paired request")??;
    assert!(entries.iter().any(|entry| entry.name == "src"));

    drop(idle_streams);
    paired.close();
    for client in idle_clients {
        client.close();
    }
    fixture.handle.shutdown().await;
    Ok(())
}
