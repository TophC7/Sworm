#![allow(dead_code)] // each test binary uses a subset

use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use sworm_remote::Identity;
use sworm_server::{auth::append_authorized, serve, ServeOptions, ServerHandle};

// Each binary has one test: HOME/XDG/git remain isolated for its whole lifetime.
pub fn isolate(root: &Path) -> anyhow::Result<()> {
    let home = root.join("home");
    let git_config = root.join("gitconfig");
    fs::create_dir_all(&home)?;
    fs::write(&git_config, "")?;
    std::env::set_var("HOME", home);
    std::env::set_var("XDG_CONFIG_HOME", root.join("config-home"));
    std::env::set_var("XDG_DATA_HOME", root.join("data-home"));
    std::env::set_var("GIT_CONFIG_GLOBAL", git_config);
    std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    Ok(())
}

pub fn init_repo(path: &Path) -> anyhow::Result<()> {
    let git = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .arg(path)
        .output()?;
    anyhow::ensure!(
        git.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&git.stderr)
    );
    fs::write(path.join("hello.txt"), "sentinel\n")?;
    Ok(())
}

pub async fn start_loop(root: &Path) -> anyhow::Result<(ServerHandle, Identity, Value)> {
    let server_config = root.join("server-config");
    let server = serve(ServeOptions {
        config_dir: server_config.clone(),
        data_dir: root.join("server-data"),
        listen: Some("127.0.0.1:0".parse()?),
        config_file: None,
        web_assets_dir: None,
    })
    .await?;
    let desktop_config = root.join("config-home/sworm");
    fs::create_dir_all(&desktop_config)?;
    let remotes = json!({
        "loop": {
            "address": format!("localhost:{}", server.local_addr.port()),
            "fingerprint": server.fingerprint.to_string(),
        }
    });
    fs::write(
        desktop_config.join("settings.jsonc"),
        serde_json::to_vec(&json!({ "remotes": remotes }))?,
    )?;
    let identity = Identity::load_or_generate(&desktop_config, "client")?;
    append_authorized(&server_config, identity.fingerprint(), "router-test")?;
    Ok((server, identity, remotes))
}
