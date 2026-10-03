#![allow(dead_code)] // each test binary uses a subset

use anyhow::{bail, Context, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc,
};
use sworm_remote::{Identity, RemoteClient, RemoteError};
use sworm_server::{auth, serve, ServeOptions, ServerHandle};
use tempfile::TempDir;

pub const FAKE_LSP_SERVER_ID: &str = "dev.sworm.nix::nil";

pub fn serve_dirs(config: &Path, data: &Path) -> ServeOptions {
    ServeOptions {
        config_dir: config.to_path_buf(),
        data_dir: data.to_path_buf(),
        listen: Some("127.0.0.1:0".parse().unwrap()),
        config_file: None,
        web_assets_dir: None,
    }
}

pub struct Daemon {
    pub config: TempDir,
    pub data: TempDir,
    pub endpoint: quinn::Endpoint,
    pub identity: Identity,
    pub handle: ServerHandle,
}

impl Daemon {
    pub async fn start(server_config: Option<serde_json::Value>) -> Result<Self> {
        let config = tempfile::tempdir()?;
        let data = tempfile::tempdir()?;
        let identity_dir = tempfile::tempdir()?;
        if let Some(contents) = server_config {
            fs::write(config.path().join("server.jsonc"), contents.to_string())?;
        }
        let identity = Identity::load_or_generate(identity_dir.path(), "client")?;
        let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
        let handle = serve(serve_dirs(config.path(), data.path())).await?;
        Ok(Self {
            config,
            data,
            endpoint,
            identity,
            handle,
        })
    }

    pub async fn client(&self) -> Result<RemoteClient, RemoteError> {
        RemoteClient::connect(
            &self.endpoint,
            self.handle.local_addr,
            &self.identity,
            self.handle.fingerprint,
        )
        .await
    }

    pub async fn client_as_new_identity(&self) -> Result<RemoteClient> {
        let dir = tempfile::tempdir()?;
        let identity = Identity::load_or_generate(dir.path(), "client")?;
        Ok(RemoteClient::connect(
            &self.endpoint,
            self.handle.local_addr,
            &identity,
            self.handle.fingerprint,
        )
        .await?)
    }

    pub async fn paired_client(&self) -> Result<RemoteClient> {
        let token = auth::write_pairing_token(self.config.path())?;
        let client = self.client().await?;
        client.pair(&token, "test client").await?;
        Ok(client)
    }

    pub async fn restart(&mut self) -> Result<()> {
        let handle = serve(serve_dirs(self.config.path(), self.data.path())).await?;
        let previous = std::mem::replace(&mut self.handle, handle);
        previous.shutdown().await;
        Ok(())
    }
}

pub fn init_repo(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    let status = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init"])
        .current_dir(path)
        .status()
        .context("launch git init")?;
    if !status.success() {
        bail!("git init failed with {status}");
    }
    fs::write(path.join("hello.txt"), "sentinel\n")?;
    fs::create_dir_all(path.join("src"))?;
    fs::write(path.join("src/lib.rs"), "pub fn sentinel() {}\n")?;
    fs::create_dir_all(path.join(".sworm"))?;
    // Harness configuration is not part of working-tree assertions.
    fs::write(path.join(".git/info/exclude"), ".sworm/\n")?;
    fs::write(
        path.join(".sworm/settings.jsonc"),
        r#"{"providers":{"terminal":{"enabled":true,"binary_path_override":"sh","extra_args":[]}}}"#,
    )?;
    Ok(())
}

pub fn once_task(command: &str) -> String {
    serde_json::json!({"version":1,"tasks":[{"id":"once","label":"Once","command":command,"singleton":true}]}).to_string()
}

pub fn mkfifo(path: &Path) -> Result<()> {
    let status = Command::new("mkfifo").arg(path).status()?;
    if !status.success() {
        bail!("mkfifo failed with {status}");
    }
    Ok(())
}

/// Reports the FIFO open, so callers know the daemon reached its blocking read.
pub struct StalledTask {
    path: PathBuf,
    release: Option<mpsc::Sender<()>>,
    writer: Option<std::thread::JoinHandle<std::io::Result<()>>>,
}

impl StalledTask {
    pub fn new(path: PathBuf, command: &str) -> Result<(Self, tokio::sync::oneshot::Receiver<()>)> {
        mkfifo(&path)?;
        let contents = once_task(command);
        let (opened, reached) = tokio::sync::oneshot::channel();
        let (release, resume) = mpsc::channel();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            let mut fifo = OpenOptions::new().write(true).open(&writer_path)?;
            let _ = opened.send(());
            let _ = resume.recv();
            fifo.write_all(contents.as_bytes())
        });
        Ok((
            Self {
                path,
                release: Some(release),
                writer: Some(writer),
            },
            reached,
        ))
    }

    pub fn unblock(&mut self) -> Result<()> {
        self.release.take();
        self.writer
            .take()
            .expect("FIFO writer already joined")
            .join()
            .expect("FIFO writer panicked")?;
        Ok(())
    }
}

impl Drop for StalledTask {
    fn drop(&mut self) {
        self.release.take();
        if let Some(writer) = self.writer.take() {
            // Linux RDWR opens without a peer, even if the daemon never read.
            let _ = OpenOptions::new().read(true).write(true).open(&self.path);
            let _ = writer.join();
        }
    }
}

pub fn enable_fake_lsp(repo: &Path) -> Result<()> {
    let script = repo.join("fake-lsp.sh");
    fs::write(
        &script,
        r#"#!/bin/sh
say() {
  printf 'Content-Length: %s\r\n\r\n%s' "${#1}" "$1"
}
say '{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"fake-lsp-started"}}'
while IFS= read -r line; do
  case "$line" in
    *initialize*) say '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}}' ;;
  esac
done
"#,
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    fs::write(repo.join(".sworm/settings.jsonc"), serde_json::json!({
        "providers": {"terminal": {"enabled": true, "binary_path_override": "sh", "extra_args": []}},
        "lsp": {"servers": {FAKE_LSP_SERVER_ID: {"enabled": true, "binary_path_override": script}}}
    }).to_string())?;
    Ok(())
}

#[path = "../../src/test_support.rs"]
pub mod test_support;
