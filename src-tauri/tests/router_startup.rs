use std::{fs, sync::Arc};
use sworm_core::{events::EventSink, events::HostEvent, Host};
use sworm_lib::router::WorkspaceRouter;
use tempfile::tempdir;

/// Tauri builds the router inside `setup`, which runs on the main thread with
/// no Tokio runtime. Binding a QUIC socket there panics, so construction must
/// stay runtime-free. Deliberately not a `#[tokio::test]`.
#[test]
fn router_constructs_without_a_tokio_runtime() -> anyhow::Result<()> {
    let temporary = tempdir()?;
    let home = temporary.path().join("home");
    fs::create_dir_all(&home)?;
    std::env::set_var("XDG_CONFIG_HOME", temporary.path().join("config-home"));
    std::env::set_var("XDG_DATA_HOME", temporary.path().join("data-home"));
    std::env::set_var("HOME", &home);

    let events: EventSink<HostEvent> = Arc::new(|_| Ok(()));
    let host = Arc::new(Host::new(
        temporary.path().join("sworm.db"),
        Arc::clone(&events),
    )?);
    let router = WorkspaceRouter::with_events(host, events);
    assert!(router.server_for_run("no-such-run").is_none());
    Ok(())
}
