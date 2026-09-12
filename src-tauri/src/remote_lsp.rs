use crate::router::{remote_error, RouterInner, Target};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Weak,
    },
    time::Duration,
};
use sworm_core::{errors::ApiError, events::EventSink};
use sworm_protocol::{
    lsp::LspEvent,
    rpc::{LspDown, LspUp, Open},
};
use sworm_remote::wire::{read_frame, write_frame};
use tokio::{sync::mpsc, task::JoinHandle, time::timeout};

/// Client messages queued while the stream writer is busy. Deep enough for a
/// burst of `didChange` notifications, shallow enough to fail fast on a dead link.
const UP_QUEUE_CAPACITY: usize = 256;
/// The daemon answers `Ready` as soon as it registers the session, so this only
/// bounds a hung connection.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// What a session's slot owns, from the instant it is claimed to the instant
/// the stream is gone.
enum SessionState {
    /// Claimed before the stream exists: a concurrent `attach` is rejected
    /// instead of overwriting a session whose stream is still opening.
    Opening,
    Running {
        up: mpsc::Sender<String>,
        /// Aborting the pump drops the send stream, which is the daemon's kill
        /// signal, and aborts the stream's reader with it.
        task: JoinHandle<()>,
    },
    /// The stream ended, so the remote server died with it. The slot is kept:
    /// this session id must report the failure rather than let its traffic fall
    /// through to the local host, which has no server for it.
    Ended,
}

/// One stream's exclusive claim on a session id.
struct RemoteLspSession {
    server: String,
    owner_id: Option<String>,
    /// Teardown is applied by lease, never by session id alone, so a dying
    /// stream can never stop or unregister its replacement.
    lease: u64,
    state: SessionState,
}

/// LSP sessions running on a daemon, each one owning a QUIC stream whose close
/// kills the remote server. Sessions are never resumed: Monaco re-initializes.
pub(crate) struct RemoteLspService {
    sessions: Mutex<HashMap<String, RemoteLspSession>>,
    next_lease: AtomicU64,
}

impl RemoteLspService {
    pub(crate) fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            next_lease: AtomicU64::new(0),
        }
    }

    /// Claim the session id, open its stream and register it before the caller
    /// starts the server, so the daemon always has a sink for the events it is
    /// about to emit. A session id whose stream is still live is refused: the
    /// client stops a session before restarting it, and nothing resumes.
    pub(crate) async fn attach(
        &self,
        router: &Arc<RouterInner>,
        session_id: &str,
        server: &str,
        owner_id: Option<String>,
        events: EventSink<LspEvent>,
    ) -> Result<(), ApiError> {
        let lease = self.claim(session_id, server, owner_id)?;
        let result = self.open(router, session_id, server, lease, events).await;
        if result.is_err() {
            self.release(session_id, lease);
        }
        result
    }

    /// Take the slot before the first `await`: a teardown that races the open
    /// then either finds this lease and revokes it, or finds nothing and leaves
    /// the replacement alone. Overwriting the map after the awaits could not.
    fn claim(
        &self,
        session_id: &str,
        server: &str,
        owner_id: Option<String>,
    ) -> Result<u64, ApiError> {
        let mut sessions = self.sessions.lock();
        if let Some(session) = sessions.get(session_id) {
            if !matches!(session.state, SessionState::Ended) {
                // Same shape as the local host's refusal: a reloaded webview
                // leaves its own stream running, and only the id's owner may
                // replace it.
                return Err(ApiError::LspAlreadyActive {
                    session_id: session_id.to_owned(),
                });
            }
        }
        let lease = self.next_lease.fetch_add(1, Ordering::Relaxed);
        sessions.insert(
            session_id.to_owned(),
            RemoteLspSession {
                server: server.to_owned(),
                owner_id,
                lease,
                state: SessionState::Opening,
            },
        );
        Ok(lease)
    }

    async fn open(
        &self,
        router: &Arc<RouterInner>,
        session_id: &str,
        server: &str,
        lease: u64,
        events: EventSink<LspEvent>,
    ) -> Result<(), ApiError> {
        let client = router.client(server).await?;
        let (send, mut recv) = client
            .open_stream(Open::Lsp {
                session_id: session_id.to_owned(),
            })
            .await
            .map_err(|error| remote_error(server, error))?;
        match timeout(READY_TIMEOUT, read_frame::<LspDown>(&mut recv)).await {
            Ok(Ok(LspDown::Ready)) => {}
            Ok(Ok(LspDown::Event { event })) => {
                return Err(ApiError::Remote(format!(
                    "{server}: LSP stream answered with an event before it was ready: {event:?}"
                )))
            }
            Ok(Err(error)) => {
                router.evict(server, &client).await;
                return Err(remote_error(server, error));
            }
            Err(_) => {
                return Err(ApiError::Remote(format!(
                    "{server}: LSP stream did not open in time"
                )))
            }
        }

        let (up, up_rx) = mpsc::channel(UP_QUEUE_CAPACITY);
        let mut sessions = self.sessions.lock();
        match sessions.get_mut(session_id) {
            Some(session) if session.lease == lease => {
                // Spawned while the slot is locked: the pump can finish before
                // this returns, and its teardown must see the registration it
                // is undoing instead of racing ahead of it.
                session.state = SessionState::Running {
                    up,
                    task: tokio::spawn(pump(
                        Arc::downgrade(router),
                        session_id.to_owned(),
                        lease,
                        send,
                        recv,
                        up_rx,
                        events,
                    )),
                };
                Ok(())
            }
            // Cancelled or window-closed while the stream opened. Dropping the
            // stream here is the daemon's kill signal, so nothing is stranded.
            _ => Err(ApiError::Remote(format!(
                "{server}: LSP session {session_id} was released while it started"
            ))),
        }
    }

    /// `None` when this session is local, so the caller falls back to `Host`.
    /// A claimed session always answers, even when its stream is gone: routing
    /// remote traffic to the local host would silently start a second server.
    pub(crate) fn send(
        &self,
        session_id: &str,
        message_json: &str,
    ) -> Option<Result<(), ApiError>> {
        let sessions = self.sessions.lock();
        let session = sessions.get(session_id)?;
        Some(match &session.state {
            SessionState::Running { up, .. } => {
                up.try_send(message_json.to_owned()).map_err(|error| {
                    ApiError::Remote(format!("remote LSP session is unavailable: {error}"))
                })
            }
            SessionState::Opening => Err(ApiError::Remote(format!(
                "remote LSP session {session_id} is still starting"
            ))),
            SessionState::Ended => Err(ApiError::Remote(format!(
                "remote LSP session {session_id} has ended"
            ))),
        })
    }

    pub(crate) fn server_for(&self, session_id: &str) -> Option<String> {
        self.sessions
            .lock()
            .get(session_id)
            .map(|session| session.server.clone())
    }

    /// Drop the stream. The daemon kills the server when it closes, so this is
    /// both the local teardown and the remote one.
    pub(crate) fn cancel(&self, session_id: &str) {
        let released = self.sessions.lock().remove(session_id);
        if let Some(session) = released {
            session.abort();
        }
    }

    /// Drop a session whose stream already ended, reporting whether that was
    /// its state. Nothing is left to stop: the daemon killed the server when
    /// the stream closed, and a stop sent for this id now could only reach the
    /// session that replaced it.
    pub(crate) fn cancel_if_ended(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock();
        if sessions
            .get(session_id)
            .is_some_and(|session| matches!(session.state, SessionState::Ended))
        {
            sessions.remove(session_id);
            return true;
        }
        false
    }

    /// Window-close parity with `Host::lsp.kill_owner`: a closed window's LSP
    /// servers die with it, wherever they run. Session ids are per window, so
    /// this never reaches another window's servers.
    pub(crate) fn release_owner(&self, owner_id: &str) {
        let released: Vec<RemoteLspSession> = {
            let mut sessions = self.sessions.lock();
            let ids: Vec<String> = sessions
                .iter()
                .filter(|(_, session)| session.owner_id.as_deref() == Some(owner_id))
                .map(|(session_id, _)| session_id.clone())
                .collect();
            ids.iter().filter_map(|id| sessions.remove(id)).collect()
        };
        for session in released {
            session.abort();
        }
    }

    /// Give up a lease that never reached `Running`; a newer lease keeps its slot.
    fn release(&self, session_id: &str, lease: u64) {
        let mut sessions = self.sessions.lock();
        if sessions
            .get(session_id)
            .is_some_and(|session| session.lease == lease)
        {
            sessions.remove(session_id);
        }
    }

    /// Record that this lease's stream is gone. The slot stays claimed until the
    /// client stops the session or its window closes.
    fn end(&self, session_id: &str, lease: u64) {
        let mut sessions = self.sessions.lock();
        if let Some(session) = sessions.get_mut(session_id) {
            if session.lease == lease {
                session.state = SessionState::Ended;
            }
        }
    }
}

impl RemoteLspSession {
    fn abort(self) {
        if let SessionState::Running { task, .. } = self.state {
            task.abort();
        }
    }
}

impl Drop for RemoteLspService {
    fn drop(&mut self) {
        for (_, session) in self.sessions.get_mut().drain() {
            session.abort();
        }
    }
}

/// Abort a task when its owner is dropped. `cancel` aborts the pump at an await
/// point, and a bare `JoinHandle` would leave its reader detached and holding
/// the stream open.
struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Strip the workspace URI from a root path: LSP servers only ever see
/// daemon-absolute paths.
pub(crate) fn daemon_root_path(server: &str, root_path: &str) -> String {
    match Target::parse(root_path) {
        Ok(Target::Remote {
            server: owner,
            path,
        }) if owner == server => path.to_owned(),
        _ => root_path.to_owned(),
    }
}

/// Write client messages upstream until the session ends. Down frames are read
/// in their own task so a queued message can never cancel a half-read frame.
async fn pump(
    router: Weak<RouterInner>,
    session_id: String,
    lease: u64,
    mut send: quinn::SendStream,
    recv: quinn::RecvStream,
    mut up_rx: mpsc::Receiver<String>,
    events: EventSink<LspEvent>,
) {
    // The server's exit reaches the client exactly once: the daemon forwards it
    // before closing the stream, and this task only invents one when it did not.
    let exited = Arc::new(AtomicBool::new(false));
    let mut down = AbortOnDrop(tokio::spawn(read_down(
        session_id.clone(),
        recv,
        Arc::clone(&events),
        Arc::clone(&exited),
    )));

    loop {
        tokio::select! {
            message = up_rx.recv() => {
                let Some(payload_json) = message else { break };
                if let Err(error) = write_frame(&mut send, &LspUp::Message { payload_json }).await {
                    tracing::warn!(%session_id, %error, "failed to send LSP message");
                    break;
                }
            }
            _ = &mut down.0 => break,
        }
    }

    // Stop reading before the stream goes away, here and on abort alike.
    drop(down);
    // Finishing the stream is the daemon's kill signal for this server.
    let _ = send.finish();
    // No resume: tell the client this session is over so it starts a new one.
    if !exited.load(Ordering::Acquire) {
        let _ = (events)(LspEvent::Exit {
            session_id: session_id.clone(),
            code: None,
        });
    }
    if let Some(router) = router.upgrade() {
        router.remote_lsp.end(&session_id, lease);
    }
}

async fn read_down(
    session_id: String,
    mut recv: quinn::RecvStream,
    events: EventSink<LspEvent>,
    exited: Arc<AtomicBool>,
) {
    loop {
        match read_frame::<LspDown>(&mut recv).await {
            Ok(LspDown::Event { event }) => {
                // Claim the exit before forwarding: a delivery failure must not
                // let the pump emit a second one.
                if matches!(event, LspEvent::Exit { .. }) {
                    exited.store(true, Ordering::Release);
                }
                if (events)(event).is_err() {
                    // The channel is gone: nothing is listening any more.
                    return;
                }
            }
            Ok(LspDown::Ready) => {}
            Err(error) => {
                tracing::debug!(%session_id, %error, "remote LSP stream ended");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_of(service: &RemoteLspService, session_id: &str) -> Option<&'static str> {
        service
            .sessions
            .lock()
            .get(session_id)
            .map(|session| match session.state {
                SessionState::Opening => "opening",
                SessionState::Running { .. } => "running",
                SessionState::Ended => "ended",
            })
    }

    #[test]
    fn a_live_session_id_cannot_be_claimed_twice() {
        let service = RemoteLspService::new();
        let lease = service
            .claim("gopls:/w", "daemon", Some("window-a".to_owned()))
            .expect("first claim");

        service
            .claim("gopls:/w", "daemon", Some("window-b".to_owned()))
            .expect_err("a second connection must not steal a live session");

        service.end("gopls:/w", lease);
        service
            .claim("gopls:/w", "daemon", Some("window-b".to_owned()))
            .expect("a dead session's id is free again");
    }

    #[test]
    fn a_stale_stream_cannot_release_its_replacement() {
        let service = RemoteLspService::new();
        let stale = service.claim("gopls:/w", "daemon", None).expect("claim");
        service.end("gopls:/w", stale);
        let fresh = service.claim("gopls:/w", "daemon", None).expect("reclaim");

        // Both teardown paths the dying stream can still reach.
        service.end("gopls:/w", stale);
        service.release("gopls:/w", stale);

        assert_eq!(state_of(&service, "gopls:/w"), Some("opening"));
        assert_eq!(
            service.sessions.lock().get("gopls:/w").map(|s| s.lease),
            Some(fresh)
        );
    }

    #[test]
    fn a_claimed_session_never_falls_back_to_the_local_host() {
        let service = RemoteLspService::new();
        let lease = service.claim("gopls:/w", "daemon", None).expect("claim");

        assert!(matches!(service.send("gopls:/w", "{}"), Some(Err(_))));
        service.end("gopls:/w", lease);
        assert!(matches!(service.send("gopls:/w", "{}"), Some(Err(_))));

        service.cancel("gopls:/w");
        assert!(service.send("gopls:/w", "{}").is_none());
        assert_eq!(state_of(&service, "gopls:/w"), None);
    }

    #[test]
    fn a_dead_stream_needs_no_stop_request() {
        let service = RemoteLspService::new();
        let lease = service.claim("gopls:/w", "daemon", None).expect("claim");

        assert!(
            !service.cancel_if_ended("gopls:/w"),
            "an opening session still owns a stream"
        );
        service.end("gopls:/w", lease);
        assert!(service.cancel_if_ended("gopls:/w"));
        assert!(
            !service.cancel_if_ended("gopls:/w"),
            "the slot is gone, so a repeat stop must not claim a dead session"
        );
        assert!(service.send("gopls:/w", "{}").is_none());
    }
}
