use crate::dispatch::ServerContext;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use sworm_core::{events::EventSink, Host};
use sworm_protocol::{
    lsp::LspEvent,
    rpc::{LspDown, LspUp},
};
use sworm_remote::wire::{read_frame, write_frame};
use tokio::{
    sync::{mpsc, watch},
    task::AbortHandle,
    time::timeout,
};

/// Bound the events a stalled desktop can queue. LSP traffic is request/reply
/// plus diagnostics: a reader this far behind is gone, not slow.
const EVENT_QUEUE_CAPACITY: usize = 1024;

/// How long a refused stream waits for the desktop to read why it was refused.
const REFUSAL_ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// Exclusive leases on the LSP sessions this daemon is streaming, keyed by
/// session id.
///
/// A lease is the server's lifetime and belongs to exactly one stream on
/// exactly one connection: `lsp_start` only finds a sink through the lease its
/// own connection holds, and only the lease holder may stop the session. One
/// live stream per session id, so a stream that is tearing down can never stop
/// or read a replacement's server, and no connection can reach another's.
pub(crate) struct LspStreams {
    leases: Arc<Leases>,
}

struct Leases {
    entries: Mutex<HashMap<String, Lease>>,
    next_token: AtomicU64,
}

struct Lease {
    /// The connection's subscriber id: session ownership is per connection.
    owner: String,
    /// Identifies this lease across reuses of the session id.
    token: u64,
    events: mpsc::Sender<LspEvent>,
    /// Set when an event could not be queued. LSP has no replay, so a dropped
    /// frame either hangs a request forever or desynchronizes an incrementally
    /// synced document: the stream ends instead, and the desktop starts over.
    overflowed: Arc<AtomicBool>,
}

/// The sink `lsp_start` hands to a spawned server, tagged with the lease that
/// issued it so a start finishing after its stream died is detectable.
pub(crate) struct LspSink {
    pub(crate) events: EventSink<LspEvent>,
    pub(crate) token: u64,
}

impl LspStreams {
    pub(crate) fn new() -> Self {
        Self {
            leases: Arc::new(Leases {
                entries: Mutex::new(HashMap::new()),
                next_token: AtomicU64::new(1),
            }),
        }
    }

    /// `None` when this connection holds no lease for the session: starting
    /// then would either spawn a process nobody could reach or kill, or hand
    /// another connection's session to this one.
    pub(crate) fn sink(&self, session_id: &str, owner: &str) -> Option<LspSink> {
        let entries = self.leases.entries.lock();
        let lease = entries.get(session_id)?;
        if lease.owner != owner {
            return None;
        }
        let token = lease.token;
        let sender = lease.events.clone();
        let overflowed = Arc::clone(&lease.overflowed);
        Some(LspSink {
            events: Arc::new(move |event| {
                sender.try_send(event).map_err(|error| {
                    // Nothing recovers a lost LSP frame, so record it for the
                    // stream loop to end the session on.
                    overflowed.store(true, Ordering::Release);
                    format!("LSP stream is not keeping up: {error}")
                })
            }),
            token,
        })
    }

    /// Whether this connection still holds the session's lease. `lsp_stop`
    /// authorization: stopping is killing, so a foreign caller must be refused.
    pub(crate) fn owns(&self, session_id: &str, owner: &str) -> bool {
        self.leases
            .entries
            .lock()
            .get(session_id)
            .is_some_and(|lease| lease.owner == owner)
    }

    /// Whether the lease that issued `token` is still the live one.
    pub(crate) fn is_current(&self, session_id: &str, token: u64) -> bool {
        self.leases
            .entries
            .lock()
            .get(session_id)
            .is_some_and(|lease| lease.token == token)
    }

    /// Take the session's lease, or `None` when another stream still holds it.
    fn claim(
        &self,
        host: Arc<Host>,
        session_id: &str,
        owner: &str,
        events: mpsc::Sender<LspEvent>,
        overflowed: Arc<AtomicBool>,
    ) -> Option<LspLease> {
        let mut entries = self.leases.entries.lock();
        if entries.contains_key(session_id) {
            return None;
        }
        let token = self.leases.next_token.fetch_add(1, Ordering::Relaxed);
        entries.insert(
            session_id.to_owned(),
            Lease {
                owner: owner.to_owned(),
                token,
                events,
                overflowed,
            },
        );
        Some(LspLease {
            leases: Arc::clone(&self.leases),
            host,
            session_id: session_id.to_owned(),
            token,
            input: None,
        })
    }
}

/// One stream's lease. Dropping it — by teardown, cancellation, or a runtime
/// that is going away mid-stream — aborts the stream's reader task and kills
/// the session's server, so neither can outlive the stream that owns them.
struct LspLease {
    leases: Arc<Leases>,
    host: Arc<Host>,
    session_id: String,
    token: u64,
    input: Option<AbortHandle>,
}

impl LspLease {
    /// Tie the stream's reader task to the lease, so cancelling the stream
    /// cannot leave a task that keeps writing to the server's stdin.
    fn owns_input(&mut self, input: AbortHandle) {
        self.input = Some(input);
    }
}

impl Drop for LspLease {
    fn drop(&mut self) {
        if let Some(input) = self.input.take() {
            input.abort();
        }
        let mut entries = self.leases.entries.lock();
        if !entries
            .get(&self.session_id)
            .is_some_and(|lease| lease.token == self.token)
        {
            return;
        }
        // Kill while still holding the lease: no replacement can claim this
        // session id between the kill and the release, so a teardown can only
        // ever kill the server it started. The connection's PTYs are untouched
        // — they outlive the desktop by design.
        let _ = self.host.lsp_kill(&self.session_id);
        entries.remove(&self.session_id);
    }
}

pub(crate) async fn run(
    host: Arc<Host>,
    context: Arc<ServerContext>,
    session_id: String,
    owner: String,
    mut send: quinn::SendStream,
    recv: quinn::RecvStream,
    mut shutdown: watch::Receiver<bool>,
) {
    let (sender, mut events) = mpsc::channel(EVENT_QUEUE_CAPACITY);
    let overflowed = Arc::new(AtomicBool::new(false));
    // A second stream for a live session is refused rather than allowed to
    // take over: the desktop stops a session before it restarts it, and a
    // silent takeover would leave the first stream reading someone else's
    // server. No lease claimed here means nothing to unwind either.
    let Some(mut lease) = context.lsp.claim(
        Arc::clone(&host),
        &session_id,
        &owner,
        sender.clone(),
        Arc::clone(&overflowed),
    ) else {
        refuse(&mut send, &session_id).await;
        return;
    };

    // Announcing readiness only after the lease is held keeps `lsp_start` from
    // racing the registration it depends on.
    if write_frame(&mut send, &LspDown::Ready).await.is_err() {
        return;
    }

    // Client messages are read in their own task so a queued event can never
    // cancel a half-read frame and desynchronize the stream.
    let mut input = tokio::spawn(read_input(
        Arc::clone(&host),
        session_id.clone(),
        recv,
        sender,
    ));
    lease.owns_input(input.abort_handle());

    loop {
        // Writes happen after the select so `stopped()` and `write_frame` never
        // borrow the send stream at the same time.
        let received = tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
                continue;
            }
            event = events.recv() => event,
            _ = &mut input => break,
            _ = send.stopped() => break,
        };
        // Ending beats delivering a gap: the desktop reads a dead stream as an
        // exit and starts a fresh session, where a silently missing frame would
        // hang a request or leave the server editing text it never received.
        if overflowed.load(Ordering::Acquire) {
            break;
        }
        let Some(event) = received else { break };
        let exited = matches!(&event, LspEvent::Exit { .. });
        if write_frame(&mut send, &LspDown::Event { event })
            .await
            .is_err()
        {
            break;
        }
        // The server is gone; the desktop starts a new session if it needs one.
        if exited {
            break;
        }
    }

    // Dropping the lease aborts the reader task, kills this session's server
    // and releases the session id, in that order.
    drop(lease);
}

/// Turn away a stream whose session id is already leased. A dropped QUIC send
/// stream resets, so the explanation is finished and given a moment to be read
/// — otherwise the desktop would only see a dead stream.
async fn refuse(send: &mut quinn::SendStream, session_id: &str) {
    let refusal = LspDown::Event {
        event: LspEvent::Error {
            session_id: session_id.to_owned(),
            message: format!("LSP session {session_id} already has an open stream"),
        },
    };
    if write_frame(send, &refusal).await.is_err() || send.finish().is_err() {
        return;
    }
    let _ = timeout(REFUSAL_ACK_TIMEOUT, send.stopped()).await;
}

/// Forward client messages to the server's stdin in arrival order.
async fn read_input(
    host: Arc<Host>,
    session_id: String,
    mut recv: quinn::RecvStream,
    events: mpsc::Sender<LspEvent>,
) {
    loop {
        match read_frame::<LspUp>(&mut recv).await {
            Ok(LspUp::Message { payload_json }) => {
                if let Err(error) = host.lsp_send(session_id.clone(), payload_json).await {
                    // stdin is gone: report it once and let the stream end.
                    let _ = events
                        .send(LspEvent::Error {
                            session_id: session_id.clone(),
                            message: error.to_string(),
                        })
                        .await;
                    return;
                }
            }
            Err(error) => {
                tracing::debug!(%session_id, %error, "LSP stream input ended");
                return;
            }
        }
    }
}
