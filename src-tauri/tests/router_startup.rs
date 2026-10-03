mod common;

use std::sync::Arc;
use sworm_core::{events::EventSink, Host};
use sworm_lib::host_events::DesktopEvent;
use sworm_lib::router::WorkspaceRouter;
use tempfile::tempdir;

/// Tauri builds the router inside `setup`, which runs on the main thread with
/// no Tokio runtime. Binding a QUIC socket there panics, so construction must
/// stay runtime-free. Deliberately not a `#[tokio::test]`.
#[test]
fn router_constructs_without_a_tokio_runtime() -> anyhow::Result<()> {
    let temporary = tempdir()?;
    common::isolate(temporary.path())?;

    let events: EventSink<DesktopEvent> = Arc::new(|_| Ok(()));
    let desktop = Arc::clone(&events);
    let host = Arc::new(Host::new(
        temporary.path().join("sworm.db"),
        Arc::new(move |event| desktop(DesktopEvent::Host(event))),
    )?);
    let router = WorkspaceRouter::with_events(host, events);
    assert!(router.server_for_run("no-such-run").is_none());
    Ok(())
}
