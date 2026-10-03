mod pairing;

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};
use sworm_core::services::settings::SettingsService;
use sworm_protocol::settings::GLOBAL_SETTINGS_DIR_NAME;
use sworm_remote::Identity;
use sworm_server::{serve, ServeOptions};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "sworm-server")]
struct Cli {
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    /// Server config; defaults to `<config-dir>/server.jsonc`.
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Web frontend used when `web.assets_dir` is unset; packaged launchers set this.
    #[arg(long, global = true)]
    web_assets_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    Pair {
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    /// Print the server's fingerprint, or that of an identity file (like `ssh-keygen -lf`).
    Fingerprint { file: Option<PathBuf> },
    /// Write a new identity to FILE and print its fingerprint (like `ssh-keygen -f`).
    /// Works for desktops too: provision it as `~/.config/sworm/client.pem`.
    Keygen { file: PathBuf },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_dir = cli
        .config_dir
        .map(Ok)
        .unwrap_or_else(SettingsService::global_config_dir)
        .map_err(anyhow::Error::msg)?;
    let config_file = cli
        .config
        .unwrap_or_else(|| sworm_server::config::default_path(&config_dir));

    match cli.command {
        Command::Serve { listen } => {
            tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                    EnvFilter::new("sworm_server=info,sworm_core=info,sworm_remote=info")
                }))
                .init();
            let data_dir = cli
                .data_dir
                .map(Ok)
                .unwrap_or_else(default_data_dir)
                .map_err(anyhow::Error::msg)?;
            let handle = serve(ServeOptions {
                config_dir,
                data_dir,
                config_file: Some(config_file),
                listen,
                web_assets_dir: cli.web_assets_dir,
            })
            .await?;
            tracing::info!("listening on {}", handle.local_addr);
            if let Some(addr) = handle.web_addr {
                tracing::warn!(%addr, "web has no authentication and grants the server OS user's filesystem and process authority");
                if !addr.ip().is_loopback() {
                    tracing::warn!(%addr, "web is exposed on a non-loopback address without authentication");
                }
                tracing::info!("web listening on http://{addr}");
            }
            tracing::info!("fingerprint {}", handle.fingerprint);
            let signal_result = wait_for_shutdown().await;
            handle.shutdown().await;
            signal_result?;
        }
        Command::Pair { host, listen } => pairing::run(&config_dir, &config_file, host, listen)?,
        Command::Fingerprint { file } => {
            let identity = match file {
                Some(file) => Identity::load(&file)?,
                None => sworm_server::config::identity(
                    &config_dir,
                    &sworm_server::config::load(&config_file)?,
                )?,
            };
            println!("{}", identity.fingerprint());
        }
        Command::Keygen { file } => println!("{}", Identity::create(&file)?.fingerprint()),
    }
    Ok(())
}

fn default_data_dir() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join(GLOBAL_SETTINGS_DIR_NAME));
    }
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(|path| {
            PathBuf::from(path)
                .join(".local/share")
                .join(GLOBAL_SETTINGS_DIR_NAME)
        })
        .ok_or_else(|| "HOME is required to resolve server data path".to_string())
}

#[cfg(unix)]
async fn wait_for_shutdown() -> anyhow::Result<()> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("install SIGTERM handler")?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result.context("wait for Ctrl-C")?,
        _ = terminate.recv() => {},
    }
    Ok(())
}

#[cfg(not(unix))]
async fn wait_for_shutdown() -> anyhow::Result<()> {
    tokio::signal::ctrl_c().await.context("wait for Ctrl-C")
}
