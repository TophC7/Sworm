use anyhow::Context;
use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};
use sworm_core::services::settings::SettingsService;
use sworm_remote::Identity;
use sworm_server::{auth, paths, serve, ServeOptions};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "sworm-server")]
struct Cli {
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        listen: Option<SocketAddr>,
    },
    Pair,
    Fingerprint,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let config_dir = cli
        .config_dir
        .map(Ok)
        .unwrap_or_else(SettingsService::global_config_dir)
        .map_err(anyhow::Error::msg)?;

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
                .unwrap_or_else(paths::default_data_dir)
                .map_err(anyhow::Error::msg)?;
            let handle = serve(ServeOptions {
                config_dir,
                data_dir,
                listen,
            })
            .await?;
            tracing::info!("listening on {}", handle.local_addr);
            tracing::info!("fingerprint {}", handle.fingerprint);
            let signal_result = wait_for_shutdown().await;
            handle.shutdown().await;
            signal_result?;
        }
        Command::Pair => {
            let identity = Identity::load_or_generate(&config_dir, "server")?;
            let token = auth::write_pairing_token(&config_dir)?;
            println!("Pairing token: {token}");
            println!("Valid for 10 minutes");
            println!("Server fingerprint: {}", identity.fingerprint());
        }
        Command::Fingerprint => {
            let identity = Identity::load_or_generate(&config_dir, "server")?;
            println!("{}", identity.fingerprint());
        }
    }
    Ok(())
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
