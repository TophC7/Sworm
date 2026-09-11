use crate::dispatch::Session;
use std::{path::Path, sync::Arc};
use sworm_core::events::HostEvent;
use sworm_protocol::rpc::HostEventWire;
use sworm_remote::wire::write_frame;
use tokio::sync::{broadcast, watch, Mutex};

pub(crate) type HostEvents = broadcast::Sender<Arc<HostEventWire>>;

pub(crate) fn to_wire(event: HostEvent) -> Option<HostEventWire> {
    match event {
        HostEvent::FilesChanged(event) => Some(HostEventWire::FilesChanged(event)),
        HostEvent::GitChanged(event) => Some(HostEventWire::GitChanged(event)),
        HostEvent::SettingsChanged(event) => Some(HostEventWire::SettingsChanged(event)),
        HostEvent::TasksChanged(folder) => Some(HostEventWire::TasksChanged(folder)),
        HostEvent::NixChanged(folder) => Some(HostEventWire::NixChanged(folder)),
        HostEvent::IssuesChanged(folder) => Some(HostEventWire::IssuesChanged(folder)),
        HostEvent::FileMoved { .. } | HostEvent::FileDeleted(_) => None,
    }
}

pub(crate) async fn run(
    session: Arc<Mutex<Session>>,
    events: HostEvents,
    connection: quinn::Connection,
    mut send: quinn::SendStream,
    mut shutdown: watch::Receiver<bool>,
) {
    let (generation, mut replaced) = {
        let mut session = session.lock().await;
        if let Some((_, stop)) = session.events.take() {
            let _ = stop.send(true);
        }
        session.next_events = session
            .next_events
            .checked_add(1)
            .expect("events stream generation overflow");
        let generation = session.next_events;
        let (stop, replaced) = watch::channel(false);
        session.events = Some((generation, stop));
        (generation, replaced)
    };
    let mut receiver = events.subscribe();

    loop {
        let received = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            changed = replaced.changed() => {
                if changed.is_err() || *replaced.borrow() {
                    break;
                }
                continue;
            }
            _ = send.stopped() => break,
            _ = connection.closed() => break,
            event = receiver.recv() => event,
        };
        let event = match received {
            Ok(event) => event,
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                tracing::warn!(skipped, "host events stream lagged");
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => break,
        };
        if !claimed(&session, event.as_ref()).await {
            continue;
        }
        // HostEventFrame is transparent; serialize the Arc payload directly to avoid a clone.
        if let Err(error) = write_frame(&mut send, event.as_ref()).await {
            tracing::debug!(%error, "host events stream closed");
            break;
        }
    }

    let mut session = session.lock().await;
    if session
        .events
        .as_ref()
        .is_some_and(|(current, _)| *current == generation)
    {
        session.events = None;
    }
    let _ = send.finish();
}

async fn claimed(session: &Mutex<Session>, event: &HostEventWire) -> bool {
    let folder = match event {
        HostEventWire::FilesChanged(event) => Some(event.folder_path.as_str()),
        HostEventWire::GitChanged(event) => Some(event.folder_path.as_str()),
        HostEventWire::SettingsChanged(event) => match event.folder_path.as_deref() {
            Some(folder) => Some(folder),
            None => return true,
        },
        HostEventWire::TasksChanged(folder)
        | HostEventWire::NixChanged(folder)
        | HostEventWire::IssuesChanged(folder) => Some(folder.as_str()),
    };
    let session = session.lock().await;
    folder.is_some_and(|folder| session.folders.contains(Path::new(folder)))
}
