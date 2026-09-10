use std::{fs, process::Command, sync::Arc, time::Duration};
use sworm_core::{errors::ApiError, Host};
use sworm_lib::router::{Target, WorkspaceRouter};
use sworm_remote::Identity;
use sworm_server::{auth::append_authorized, serve, ServeOptions};
use tempfile::tempdir;
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread")]
async fn routes_local_and_remote_workspaces_over_real_quic() -> anyhow::Result<()> {
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
    let settings_contents = format!(
        r#"{{"remotes":{{"loop":{{"address":"localhost:{}","fingerprint":"{server_fingerprint}"}}}}}}"#,
        server_address.port()
    );
    fs::write(&settings_path, &settings_contents)?;
    let desktop_identity = Identity::load_or_generate(&desktop_config, "client")?;
    append_authorized(
        &server_config,
        desktop_identity.fingerprint(),
        "router-test",
    )?;

    let host = Arc::new(Host::new(
        temporary.path().join("sworm.db"),
        Arc::new(|_| Ok(())),
    )?);
    let router = WorkspaceRouter::new(host);
    let remote_repository = format!("sworm://loop{}", repository.display());

    let remote_content = router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await?;
    assert_eq!(remote_content, "sentinel\n");

    let entries = router
        .files_read_dir(remote_repository.clone(), String::new(), false)
        .await?;
    assert!(entries.iter().any(|entry| entry.name == "hello.txt"));

    let summary = router.git_get_summary(remote_repository.clone()).await?;
    assert!(summary.is_repo);

    let local_content = router
        .file_read(
            repository.to_string_lossy().into_owned(),
            "hello.txt".into(),
        )
        .await?;
    assert_eq!(local_content, remote_content);
    fs::write(
        &settings_path,
        format!(
            r#"{{"remotes":{{"loop":{{"address":"not-a-socket","fingerprint":"{server_fingerprint}"}}}}}}"#
        ),
    )?;
    let changed_address = router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("changed address must not reuse cached client");
    assert!(changed_address.to_string().contains("cannot resolve"));

    fs::write(
        &settings_path,
        format!(r#"{{"remotes":{{"loop":{{"address":"{server_address}","fingerprint":"bad"}}}}}}"#),
    )?;
    let changed_pin = router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("changed pin must not reuse cached client");
    assert!(matches!(changed_pin, ApiError::InvalidArgument(_)));

    fs::write(&settings_path, r#"{"remotes":{}}"#)?;
    let removed = router
        .file_read(remote_repository.clone(), "hello.txt".into())
        .await
        .expect_err("removed remote must not reuse cached client");
    assert!(removed.to_string().contains("Unknown remote server"));

    fs::write(&settings_path, &settings_contents)?;
    assert_eq!(
        router
            .file_read(remote_repository.clone(), "hello.txt".into())
            .await?,
        "sentinel\n"
    );

    let unknown = router
        .file_read("sworm://nope/x".into(), "hello.txt".into())
        .await
        .expect_err("unknown remote must fail");
    assert!(unknown.to_string().contains("Unknown remote server"));

    assert!(matches!(
        Target::parse("sworm://"),
        Err(ApiError::InvalidArgument(message)) if message == "Invalid remote path: sworm://"
    ));

    server.shutdown().await;
    let stopped = timeout(
        Duration::from_secs(15),
        router.file_read(remote_repository, "hello.txt".into()),
    )
    .await
    .expect("stopped server failure exceeded connection timeout")
    .expect_err("stopped server must fail");
    assert!(matches!(stopped, ApiError::Remote(_)));

    Ok(())
}
