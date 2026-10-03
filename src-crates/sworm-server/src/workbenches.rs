//! Durable workbenches: the SQLite manifest, controller leases and the
//! serialized attach/takeover/close transitions for each workbench.
//!
//! The manifest is read and rewritten only inside writer transactions; no
//! mutable copy is cached. Runtime slots hold what cannot survive a restart:
//! the transition gate, the close fence and the current or draining control.

use crate::dispatch::ServerContext;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
};
use sworm_core::{services::app_state_kv::AppStateKvService, Host};
use sworm_protocol::rpc::{AttachMode, WireError, WorkbenchInfo, WorkbenchRun};
use tokio::sync::{watch, Mutex, OwnedMutexGuard};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) const MANIFEST_KEY: &str = "web_workbench_manifest";
const SNAPSHOT_PREFIX: &str = "workbench:";
const EMPTY_SNAPSHOT: &str = r#"{"version":4,"activeTabIndex":-1,"tabs":[]}"#;
const CLEANUP_FAILED: &str = "Workbench cleanup failed; restart the server before retrying";
const CLOSING: &str = "Workbench is closing; retry Close Workbench";

/// Why a controller stopped admitting work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Terminal {
    Revoked,
    Closed,
    Disconnected,
}

/// Admission and completion shared by browser sockets and QUIC leases.
pub(crate) struct ControlLease {
    tasks: TaskTracker,
    pub(crate) stop: watch::Sender<bool>,
    terminal: parking_lot::Mutex<Option<Terminal>>,
    completion: watch::Sender<Option<Result<(), WireError>>>,
}

impl ControlLease {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            tasks: TaskTracker::new(),
            stop: watch::channel(false).0,
            terminal: parking_lot::Mutex::new(None),
            completion: watch::channel(None).0,
        })
    }

    pub(crate) fn retire(&self, terminal: Terminal) {
        let mut current = self.terminal.lock();
        if current.is_none() {
            *current = Some(terminal);
        }
        self.tasks.close();
        self.stop.send_replace(true);
    }

    pub(crate) fn admitted(&self) -> bool {
        self.terminal.lock().is_none()
    }

    pub(crate) fn terminal(&self) -> Option<Terminal> {
        *self.terminal.lock()
    }

    pub(crate) fn admit<T>(&self, track: impl FnOnce(&TaskTracker) -> T) -> Option<T> {
        let current = self.terminal.lock();
        current.is_none().then(|| track(&self.tasks))
    }

    pub(crate) fn track(&self) -> Option<tokio_util::task::task_tracker::TaskTrackerToken> {
        self.admit(TaskTracker::token)
    }

    pub(crate) async fn drained(&self) {
        self.tasks.wait().await;
    }

    pub(crate) fn complete(&self, result: Result<(), WireError>) {
        self.completion.send_replace(Some(result));
    }

    pub(crate) async fn completion(&self) -> Result<(), WireError> {
        let mut completion = self.completion.subscribe();
        let result = completion
            .wait_for(Option::is_some)
            .await
            .expect("lease owns its completion sender")
            .clone();
        result.expect("waited for completion")
    }

    pub(crate) async fn retired(&self) {
        let mut stop = self.stop.subscribe();
        let _ = stop.wait_for(|stopped| *stopped).await;
    }
}

pub(crate) fn snapshot_id(key: &str) -> Option<&str> {
    key.strip_prefix(SNAPSHOT_PREFIX)
}

fn snapshot_key(id: &str) -> String {
    format!("{SNAPSHOT_PREFIX}{id}")
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkbenchRecord {
    id: String,
    created_at: String,
    last_seen_at: String,
    #[serde(deserialize_with = "explicit")]
    controller_token: Option<String>,
    /// Run owner for this incarnation. The Host keeps closed owners closed,
    /// so a recreated id must not reuse its predecessor's.
    owner: String,
    #[serde(default)]
    client: Option<String>,
}

/// Ids come from page URLs: short, and safe in keys and logs.
fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// A nullable field that must still be present.
pub(crate) fn explicit<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

pub(crate) enum Attach {
    Ready {
        token: String,
        lease: Arc<ControlLease>,
        owner: String,
        snapshot: String,
    },
    Busy {
        client: Option<String>,
        snapshot: String,
    },
    Revoked {
        client: Option<String>,
        snapshot: String,
    },
}

#[derive(Default)]
pub(crate) struct Workbenches {
    slots: parking_lot::Mutex<HashMap<String, Arc<Slot>>>,
    /// Transitions outlive the socket or RPC that requested them; shutdown
    /// awaits them before the Host stops.
    transitions: TaskTracker,
}

#[derive(Default)]
struct Slot {
    transition: Arc<Mutex<()>>,
    /// Raised by Close under the transition gate and never lowered; a failed
    /// Close keeps it. Observable without the gate, so a hello never queues
    /// behind a Close that is draining admitted work.
    closing: CancellationToken,
    state: parking_lot::Mutex<SlotState>,
}

#[derive(Default)]
struct SlotState {
    /// Set under the transition gate; a waiter that finds it retries lookup
    /// instead of publishing into an orphaned slot.
    removed: bool,
    /// Current or still-draining lease. Late cleanup clears only itself.
    control: Option<Arc<ControlLease>>,
}

impl Workbenches {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) async fn shutdown(&self) {
        self.transitions.close();
        self.transitions.wait().await;
    }

    fn connected(&self, id: &str) -> bool {
        self.slots.lock().get(id).is_some_and(|slot| {
            slot.state
                .lock()
                .control
                .as_ref()
                .is_some_and(|control| control.admitted())
        })
    }

    async fn lock(&self, id: &str) -> (Arc<Slot>, OwnedMutexGuard<()>) {
        loop {
            let slot = Arc::clone(self.slots.lock().entry(id.to_owned()).or_default());
            let guard = Arc::clone(&slot.transition).lock_owned().await;
            if !slot.state.lock().removed {
                return (slot, guard);
            }
        }
    }

    /// `lock` for a hello: a fenced slot rejects at once, and a hello already
    /// waiting for the gate is released as soon as Close raises the fence.
    async fn lock_open(&self, id: &str) -> Result<(Arc<Slot>, OwnedMutexGuard<()>), WireError> {
        loop {
            let slot = Arc::clone(self.slots.lock().entry(id.to_owned()).or_default());
            let guard = tokio::select! {
                biased;
                _ = slot.closing.cancelled() => return Err(closing()),
                guard = Arc::clone(&slot.transition).lock_owned() => guard,
            };
            if slot.state.lock().removed {
                continue;
            }
            // Only a gate holder raises the fence, so this check is final.
            if slot.closing.is_cancelled() {
                return Err(closing());
            }
            return Ok((slot, guard));
        }
    }

    /// Caller holds the transition gate.
    fn remove(&self, id: &str, slot: &Arc<Slot>) {
        slot.state.lock().removed = true;
        let mut slots = self.slots.lock();
        if slots
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, slot))
        {
            slots.remove(id);
        }
    }

    /// Caller holds the transition gate.
    fn remove_if_idle(&self, id: &str, slot: &Arc<Slot>) {
        let idle = !slot.closing.is_cancelled() && slot.state.lock().control.is_none();
        if idle {
            self.remove(id, slot);
        }
    }

    async fn supervise<T: Send + 'static>(
        &self,
        transition: impl Future<Output = Result<T, WireError>> + Send + 'static,
    ) -> Result<T, WireError> {
        self.transitions
            .spawn(transition)
            .await
            .map_err(|error| WireError::Internal {
                message: format!("workbench transition failed: {error}"),
            })?
    }
}

/// Wait for a retired control's admitted work and Session cleanup. A failed
/// cleanup is sticky: its slot keeps the control, so every later transition
/// observes the same result instead of repeating partial releases.
async fn drain(slot: &Slot, control: &Arc<ControlLease>) -> Result<(), WireError> {
    control
        .completion()
        .await
        .map_err(|_| WireError::Internal {
            message: CLEANUP_FAILED.to_owned(),
        })?;
    let mut state = slot.state.lock();
    if state
        .control
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, control))
    {
        state.control = None;
    }
    Ok(())
}

fn database(message: impl std::fmt::Display) -> WireError {
    WireError::Database {
        message: message.to_string(),
    }
}

fn closing() -> WireError {
    WireError::InvalidArgument {
        message: CLOSING.to_owned(),
    }
}

fn load(conn: &rusqlite::Connection) -> Result<Vec<WorkbenchRecord>, WireError> {
    let Some(raw) = AppStateKvService::get(conn, MANIFEST_KEY).map_err(database)? else {
        return Ok(Vec::new());
    };
    let records: Vec<WorkbenchRecord> = serde_json::from_str(&raw)
        .map_err(|error| database(format!("web workbench manifest is malformed: {error}")))?;
    let mut ids = HashSet::new();
    for record in &records {
        let valid = valid_id(&record.id)
            && !record.owner.is_empty()
            && ids.insert(record.id.as_str())
            && DateTime::parse_from_rfc3339(&record.created_at).is_ok()
            && DateTime::parse_from_rfc3339(&record.last_seen_at).is_ok()
            && record
                .controller_token
                .as_deref()
                .is_none_or(|token| !token.is_empty());
        if !valid {
            return Err(database(format!(
                "web workbench manifest has an invalid record: {}",
                record.id
            )));
        }
    }
    Ok(records)
}

fn store(conn: &rusqlite::Connection, records: &[WorkbenchRecord]) -> Result<(), WireError> {
    let raw = serde_json::to_string(records).map_err(database)?;
    AppStateKvService::put(conn, MANIFEST_KEY, &raw).map_err(database)
}

/// One writer transaction; dropping it on error rolls every write back.
fn transaction<T>(
    host: &Host,
    work: impl FnOnce(&rusqlite::Connection) -> Result<T, WireError>,
) -> Result<T, WireError> {
    let db = host.db.write();
    let transaction = db.conn().unchecked_transaction().map_err(database)?;
    let value = work(&transaction)?;
    transaction.commit().map_err(database)?;
    Ok(value)
}

async fn blocking<T: Send + 'static>(
    host: &Arc<Host>,
    work: impl FnOnce(&Host) -> Result<T, WireError> + Send + 'static,
) -> Result<T, WireError> {
    let host = Arc::clone(host);
    tokio::task::spawn_blocking(move || work(&host))
        .await
        .map_err(|error| WireError::Internal {
            message: format!("workbench storage task failed: {error}"),
        })?
}

fn not_found(id: &str) -> WireError {
    WireError::NotFound {
        message: format!("Workbench not found: {id}"),
    }
}

/// Drop a workbench's durable record and snapshot.
fn delete(conn: &rusqlite::Connection, id: &str) -> Result<(), WireError> {
    let mut records = load(conn)?;
    records.retain(|record| record.id != id);
    AppStateKvService::delete(conn, &snapshot_key(id)).map_err(database)?;
    store(conn, &records)
}

/// Stop every run of `owner`. The Host keeps the owner closed afterwards.
async fn stop_runs(
    host: &Arc<Host>,
    context: &ServerContext,
    owner: String,
) -> Result<(), WireError> {
    let (run_ids, stopped) = blocking(host, move |host| {
        let run_ids = host.pty.owner_run_ids(&owner, &HashSet::new());
        Ok((run_ids, host.stop_owner(&owner)))
    })
    .await?;
    context.retire_stopped_runs(host, run_ids).await?;
    stopped.map_err(WireError::from)
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn info(
    record: WorkbenchRecord,
    connected: bool,
    yours: bool,
    folders: Vec<String>,
    running: Vec<WorkbenchRun>,
) -> WorkbenchInfo {
    WorkbenchInfo {
        id: record.id,
        created_at: record.created_at,
        last_seen_at: record.last_seen_at,
        connected,
        client: record.client,
        yours,
        folders,
        running,
    }
}

/// The part of a page's workbench snapshot the registry reads; the rest of
/// each tab and the layout tree are skipped, not parsed into values.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotHead {
    version: u64,
    active_tab_index: Option<i64>,
    tabs: Vec<TabHead>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TabHead {
    folder_path: String,
}

fn snapshot_head(snapshot: &str) -> Option<SnapshotHead> {
    serde_json::from_str::<SnapshotHead>(snapshot)
        .ok()
        .filter(|head| head.version == 4)
}

/// A V4 snapshot without tabs: nothing a page could restore.
fn tabless(snapshot: &str) -> bool {
    snapshot_head(snapshot).is_some_and(|head| head.tabs.is_empty())
}

/// Tab folders of a V4 snapshot: the active tab's first, then the remaining
/// distinct non-empty ones in tab order. Anything malformed yields none.
fn folders(snapshot: &str) -> Vec<String> {
    let parse = || -> Option<Vec<String>> {
        let head = snapshot_head(snapshot)?;
        let paths: Vec<&str> = head
            .tabs
            .iter()
            .map(|tab| tab.folder_path.as_str())
            .collect();
        let active = match head.active_tab_index? {
            -1 => None,
            index => Some(*paths.get(usize::try_from(index).ok()?)?),
        };
        let mut folders: Vec<String> = Vec::new();
        for path in active.into_iter().chain(paths.iter().copied()) {
            if !path.is_empty() && !folders.iter().any(|folder| folder == path) {
                folders.push(path.to_owned());
            }
        }
        Some(folders)
    };
    parse().unwrap_or_default()
}

pub(crate) async fn list(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    yours: &HashSet<String>,
) -> Result<Vec<WorkbenchInfo>, WireError> {
    let (records, mut running) = blocking(host, |host| {
        let db = host.db.read();
        let records = load(db.conn())?
            .into_iter()
            .map(|record| {
                let snapshot = AppStateKvService::get(db.conn(), &snapshot_key(&record.id))
                    .map_err(database)?;
                Ok((record, snapshot.as_deref().map(folders).unwrap_or_default()))
            })
            .collect::<Result<Vec<_>, WireError>>()?;
        Ok((records, host.live_runs_by_owner()))
    })
    .await?;
    let mut infos: Vec<_> = records
        .into_iter()
        .map(|(record, folders)| {
            let connected = context.workbenches.connected(&record.id);
            let running = running.remove(&record.owner).unwrap_or_default();
            let yours = yours.contains(&record.id);
            info(record, connected, yours, folders, running)
        })
        .collect();
    // Validated on load; the parse cannot fail here.
    let seen = |info: &WorkbenchInfo| DateTime::parse_from_rfc3339(&info.last_seen_at).ok();
    infos.sort_by(|a, b| seen(b).cmp(&seen(a)).then_with(|| a.id.cmp(&b.id)));
    Ok(infos)
}

async fn lookup(host: &Arc<Host>, id: &str) -> Result<Option<WorkbenchRecord>, WireError> {
    let id = id.to_owned();
    blocking(host, move |host| {
        Ok(load(host.db.read().conn())?
            .into_iter()
            .find(|record| record.id == id))
    })
    .await
}

pub(crate) async fn snapshot(host: &Arc<Host>, id: &str) -> Result<String, WireError> {
    let id = id.to_owned();
    blocking(host, move |host| {
        AppStateKvService::get(host.db.read().conn(), &snapshot_key(&id))
            .map_err(database)?
            .ok_or_else(|| not_found(&id))
    })
    .await
}

pub(crate) async fn is_registered(host: &Arc<Host>, id: &str) -> Result<bool, WireError> {
    Ok(lookup(host, id).await?.is_some())
}

/// Store a controller's own snapshot and refresh last-seen. The controller
/// token is left untouched: a write admitted before a takeover must not
/// restore the loser's lease.
pub(crate) async fn put_snapshot(
    host: &Arc<Host>,
    id: String,
    value_json: String,
) -> Result<(), WireError> {
    blocking(host, move |host| {
        transaction(host, |conn| {
            let mut records = load(conn)?;
            let record = records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| not_found(&id))?;
            AppStateKvService::put(conn, &snapshot_key(&id), &value_json).map_err(database)?;
            record.last_seen_at = now();
            store(conn, &records)
        })
    })
    .await
}

/// Commit an attach: refresh last-seen, client and (when given) lease token.
/// An unknown id is created in the same transaction. Returns the run owner.
fn commit_attach(
    host: &Host,
    id: &str,
    token: Option<&str>,
    client: &str,
) -> Result<String, WireError> {
    transaction(host, |conn| {
        let mut records = load(conn)?;
        let stamp = now();
        let index = match records.iter().position(|record| record.id == id) {
            Some(index) => index,
            None => {
                AppStateKvService::put(conn, &snapshot_key(id), EMPTY_SNAPSHOT)
                    .map_err(database)?;
                records.push(WorkbenchRecord {
                    id: id.to_owned(),
                    created_at: stamp.clone(),
                    last_seen_at: stamp.clone(),
                    controller_token: None,
                    owner: uuid::Uuid::new_v4().to_string(),
                    client: None,
                });
                records.len() - 1
            }
        };
        let record = &mut records[index];
        if let Some(token) = token {
            record.controller_token = Some(token.to_owned());
        }
        record.last_seen_at = stamp;
        record.client = Some(client.to_owned());
        let owner = record.owner.clone();
        store(conn, &records)?;
        Ok(owner)
    })
}

/// Bind a new control as the workbench's controller, creating the workbench
/// when the id is unknown. A matching token resumes, a stale one is revoked,
/// and an active controller is busy unless the caller takes over.
pub(crate) async fn attach(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    id: String,
    mode: AttachMode,
    client: String,
) -> Result<Attach, WireError> {
    if !valid_id(&id) {
        return Err(WireError::InvalidArgument {
            message: "Invalid workbench id".to_owned(),
        });
    }
    let host = Arc::clone(host);
    let context = Arc::clone(context);
    context
        .clone()
        .workbenches
        .supervise(async move {
            let workbenches = &context.workbenches;
            let (slot, _transition) = workbenches.lock_open(&id).await?;
            let record = lookup(&host, &id).await?;
            let existing = slot.state.lock().control.clone();
            // An unknown id has no lease to resume: it is created fresh.
            let mode = match mode {
                AttachMode::Resume { .. } if record.is_none() => AttachMode::Open {},
                mode => mode,
            };
            let (token, owner) = match mode {
                AttachMode::Resume {
                    controller_token: token,
                } => {
                    if record
                        .as_ref()
                        .and_then(|record| record.controller_token.as_deref())
                        != Some(token.as_str())
                    {
                        let client = record.as_ref().and_then(|record| record.client.clone());
                        let snapshot = snapshot(&host, &id).await?;
                        workbenches.remove_if_idle(&id, &slot);
                        return Ok(Attach::Revoked { client, snapshot });
                    }
                    // Same lease: a half-open predecessor is replaced, never busy.
                    if let Some(old) = existing {
                        old.retire(Terminal::Revoked);
                        drain(&slot, &old).await?;
                    }
                    let attached = id.clone();
                    let label = client.clone();
                    let owner = blocking(&host, move |host| {
                        commit_attach(host, &attached, None, &label)
                    })
                    .await?;
                    (token, owner)
                }
                AttachMode::Takeover {} => {
                    // Persist first: a failed write leaves the old controller intact,
                    // and a restart can never restore the loser's token.
                    let token = uuid::Uuid::new_v4().to_string();
                    let (attached, lease) = (id.clone(), token.clone());
                    let label = client.clone();
                    let owner = blocking(&host, move |host| {
                        commit_attach(host, &attached, Some(&lease), &label)
                    })
                    .await?;
                    if let Some(old) = existing {
                        old.retire(Terminal::Revoked);
                        drain(&slot, &old).await?;
                    }
                    (token, owner)
                }
                AttachMode::Open {} => {
                    if let Some(old) = existing {
                        if old.admitted() {
                            let client = record.as_ref().and_then(|record| record.client.clone());
                            return Ok(Attach::Busy {
                                client,
                                snapshot: snapshot(&host, &id).await?,
                            });
                        }
                        drain(&slot, &old).await?;
                    }
                    let token = uuid::Uuid::new_v4().to_string();
                    let (attached, lease) = (id.clone(), token.clone());
                    let label = client.clone();
                    let owner = blocking(&host, move |host| {
                        commit_attach(host, &attached, Some(&lease), &label)
                    })
                    .await?;
                    (token, owner)
                }
            };
            let snapshot = snapshot(&host, &id).await?;
            let lease = ControlLease::new();
            slot.state.lock().control = Some(Arc::clone(&lease));
            let _ = context.host_events.send(Arc::new(
                sworm_protocol::rpc::HostEventWire::WorkbenchesChanged(()),
            ));
            Ok(Attach::Ready {
                token,
                lease,
                owner,
                snapshot,
            })
        })
        .await
}

/// Called after `control` completed. Clears only this control and, while its
/// lease is still the durable one, deletes the workbench if it was left empty
/// or else refreshes last-seen.
pub(crate) async fn detach(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    id: String,
    token: String,
    control: &Arc<ControlLease>,
) {
    let slot = context.workbenches.slots.lock().get(&id).cloned();
    let cleaned = match slot {
        Some(slot) => drain(&slot, control).await.is_ok(),
        None => control.completion().await.is_ok(),
    };
    if !cleaned {
        return;
    }
    let host = Arc::clone(host);
    let context = Arc::clone(context);
    let result = context
        .clone()
        .workbenches
        .supervise(async move {
            let workbenches = &context.workbenches;
            // Under the gate a reload's re-attach has either rebound the slot
            // already or will find the workbench gone and recreate it.
            let (slot, _transition) = workbenches.lock(&id).await;
            let rebound = slot.closing.is_cancelled() || slot.state.lock().control.is_some();
            let detached = id.clone();
            let pruned = blocking(&host, move |host| {
                transaction(host, |conn| {
                    let mut records = load(conn)?;
                    let Some(record) = records.iter_mut().find(|record| {
                        record.id == detached
                            && record.controller_token.as_deref() == Some(token.as_str())
                    }) else {
                        return Ok(None);
                    };
                    let empty = !rebound
                        && AppStateKvService::get(conn, &snapshot_key(&detached))
                            .map_err(database)?
                            .as_deref()
                            .is_some_and(tabless)
                        && !host.pty.owner_has_live_run(&record.owner);
                    if empty {
                        let owner = record.owner.clone();
                        delete(conn, &detached)?;
                        return Ok(Some(Some(owner)));
                    }
                    record.last_seen_at = now();
                    store(conn, &records)?;
                    Ok(Some(None))
                })
            })
            .await?;
            if pruned.is_some() {
                let _ = context.host_events.send(Arc::new(
                    sworm_protocol::rpc::HostEventWire::WorkbenchesChanged(()),
                ));
            }
            let Some(Some(owner)) = pruned else {
                workbenches.remove_if_idle(&id, &slot);
                return Ok(());
            };
            workbenches.remove(&id, &slot);
            // Releases finished runs; recreation mints a new owner.
            stop_runs(&host, &context, owner).await
        })
        .await;
    if let Err(error) = result {
        tracing::warn!(?error, "workbench detach failed");
    }
}

/// Explicit, destructive Close: fence, drain, stop owned runs, then delete.
/// Failures keep the durable record and the fence so Close can be retried.
///
/// Draining waits for the target page's in-flight requests, which may include
/// its own Close of `caller`. So once `caller` itself is being closed, stop
/// waiting: the supervised transition still finishes, and two pages closing
/// each other both close instead of each waiting on the other. A page closing
/// itself gets success at that point: the close was accepted, not completed.
pub(crate) async fn close(
    host: &Arc<Host>,
    context: &Arc<ServerContext>,
    id: String,
    caller: Option<&str>,
) -> Result<(), WireError> {
    let workbenches = &context.workbenches;
    let self_close = caller == Some(id.as_str());
    let caller_closing = caller.and_then(|caller| {
        workbenches
            .slots
            .lock()
            .get(caller)
            .map(|slot| slot.closing.clone())
    });
    let host = Arc::clone(host);
    let context = Arc::clone(context);
    let transition = workbenches.supervise(async move {
        let workbenches = &context.workbenches;
        // Absent is idempotent success and never allocates a slot; the gated
        // re-check covers a concurrent Close that finished first.
        if !is_registered(&host, &id).await? {
            return Ok(());
        }
        let (slot, _transition) = workbenches.lock(&id).await;
        let Some(record) = lookup(&host, &id).await? else {
            workbenches.remove_if_idle(&id, &slot);
            return Ok(());
        };
        slot.closing.cancel();
        let control = slot.state.lock().control.clone();
        if let Some(control) = control {
            control.retire(Terminal::Closed);
            drain(&slot, &control).await?;
        }
        stop_runs(&host, &context, record.owner).await?;
        let deleted = id.clone();
        blocking(&host, move |host| {
            transaction(host, |conn| delete(conn, &deleted))
        })
        .await?;
        workbenches.remove(&id, &slot);
        let _ = context.host_events.send(Arc::new(
            sworm_protocol::rpc::HostEventWire::WorkbenchesChanged(()),
        ));
        Ok(())
    });
    let Some(caller_closing) = caller_closing else {
        return transition.await;
    };
    // Biased: the transition is spawned on its first poll, before any exit.
    tokio::select! {
        biased;
        result = transition => result,
        _ = caller_closing.cancelled() => if self_close { Ok(()) } else { Err(closing()) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_lead_with_the_active_tab_then_tab_order() {
        let snapshot = |index: i64, tabs: &str| {
            format!(r#"{{"version":4,"activeTabIndex":{index},"tabs":{tabs}}}"#)
        };
        let tabs = r#"[{"folderPath":"/tmp/a"},{"folderPath":"/tmp/b"},{"folderPath":"/tmp/a"},{"folderPath":""},{"folderPath":"/tmp/c"}]"#;
        assert_eq!(folders(&snapshot(0, tabs)), ["/tmp/a", "/tmp/b", "/tmp/c"]);
        assert_eq!(folders(&snapshot(1, tabs)), ["/tmp/b", "/tmp/a", "/tmp/c"]);
        assert_eq!(folders(&snapshot(4, tabs)), ["/tmp/c", "/tmp/a", "/tmp/b"]);
        assert_eq!(folders(&snapshot(-1, tabs)), ["/tmp/a", "/tmp/b", "/tmp/c"]);
        assert_eq!(folders(&snapshot(3, tabs)), ["/tmp/a", "/tmp/b", "/tmp/c"]);
        assert!(folders(EMPTY_SNAPSHOT).is_empty());
        for malformed in [
            snapshot(5, tabs),
            snapshot(-2, tabs),
            snapshot(0, r#"[{"folderPath":"/tmp/a"},{"kind":"session"}]"#),
            snapshot(0, r#"{"folderPath":"/tmp/a"}"#),
            r#"{"version":3,"activeTabIndex":0,"tabs":[{"folderPath":"/a"}]}"#.to_owned(),
            r#"{"version":4,"tabs":[{"folderPath":"/a"}]}"#.to_owned(),
            "not json".to_owned(),
        ] {
            assert!(folders(&malformed).is_empty(), "{malformed}");
        }
    }

    async fn ready(
        host: &Arc<Host>,
        context: &Arc<ServerContext>,
        id: &str,
        token: Option<String>,
    ) -> (String, Arc<ControlLease>) {
        let mode = match token {
            Some(controller_token) => AttachMode::Resume { controller_token },
            None => AttachMode::Open {},
        };
        match attach(host, context, id.to_owned(), mode, "browser".into())
            .await
            .unwrap()
        {
            Attach::Ready { token, lease, .. } => (token, lease),
            _ => panic!("attach was not ready"),
        }
    }

    /// The owning socket's teardown after an ordinary disconnect.
    async fn disconnect(
        host: &Arc<Host>,
        context: &Arc<ServerContext>,
        id: &str,
        token: String,
        control: &Arc<ControlLease>,
    ) {
        control.retire(Terminal::Disconnected);
        control.complete(Ok(()));
        detach(host, context, id.to_owned(), token, control).await;
    }

    /// Completes `control` once a transition retires it, as its socket would.
    fn socket(control: &Arc<ControlLease>) -> tokio::task::JoinHandle<()> {
        let control = Arc::clone(control);
        tokio::spawn(async move {
            control.retired().await;
            control.complete(Ok(()));
        })
    }

    // Run in a child so HOME/XDG never mutate the lib-test process shared by
    // other concurrently running unit tests.
    #[test]
    fn close_failures_keep_the_workbench_fenced() {
        if !crate::test_support::isolated(
            "workbenches::tests::close_failures_keep_the_workbench_fenced",
        ) {
            return;
        }
        let scratch = tempfile::tempdir().unwrap();
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let db = scratch.path().join("server.db");
                let host = Arc::new(Host::new(db.clone(), Arc::new(|_| Ok(()))).unwrap());
                let (events, _) = tokio::sync::broadcast::channel(4);
                let context = Arc::new(ServerContext::new(std::path::PathBuf::new(), None, events));
                // A DB failure during deletion keeps record, snapshot and fence;
                // a second Close completes instead of waiting on a consumed drain.
                let a = "a".to_owned();
                let (_, control) = ready(&host, &context, &a, None).await;
                let first_owner = lookup(&host, &a).await.unwrap().unwrap().owner;
                let teardown = socket(&control);
                let external = rusqlite::Connection::open(&db).unwrap();
                external
                    .execute_batch(&format!(
                        "CREATE TRIGGER keep BEFORE DELETE ON app_state \
                         WHEN old.key = 'workbench:{a}' BEGIN SELECT RAISE(ABORT, 'held'); END;"
                    ))
                    .unwrap();
                assert!(matches!(
                    close(&host, &context, a.clone(), None).await,
                    Err(WireError::Database { .. })
                ));
                teardown.await.unwrap();
                assert!(is_registered(&host, &a).await.unwrap());
                assert!(host.app_state_get(snapshot_key(&a)).unwrap().is_some());
                assert_eq!(
                    attach(
                        &host,
                        &context,
                        a.clone(),
                        AttachMode::Takeover {},
                        "browser".into()
                    )
                    .await
                    .err(),
                    Some(closing())
                );
                external.execute_batch("DROP TRIGGER keep;").unwrap();
                tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    close(&host, &context, a.clone(), None),
                )
                .await
                .unwrap()
                .unwrap();
                assert!(!is_registered(&host, &a).await.unwrap());
                assert!(host.app_state_get(snapshot_key(&a)).unwrap().is_none());
                close(&host, &context, a.clone(), None).await.unwrap();
                close(&host, &context, "never-created".to_owned(), None)
                    .await
                    .unwrap();
                assert!(
                    context.workbenches.slots.lock().is_empty(),
                    "absent Close kept a slot"
                );

                // Re-attaching a closed id recreates it under a new run owner;
                // leaving it empty prunes it and its slot.
                let (token, control) = ready(&host, &context, &a, None).await;
                let recreated = lookup(&host, &a).await.unwrap().unwrap();
                assert_ne!(recreated.owner, first_owner, "closed owner was reused");
                disconnect(&host, &context, &a, token, &control).await;
                assert!(!is_registered(&host, &a).await.unwrap());
                assert!(host.app_state_get(snapshot_key(&a)).unwrap().is_none());
                assert!(
                    context.workbenches.slots.lock().is_empty(),
                    "pruned workbench kept a slot"
                );

                // A resumed lease rebinds the slot before the old detach runs,
                // so that detach keeps the workbench.
                let (token, old) = ready(&host, &context, &a, None).await;
                let teardown = socket(&old);
                let (_, new) = ready(&host, &context, &a, Some(token.clone())).await;
                teardown.await.unwrap();
                detach(&host, &context, a.clone(), token.clone(), &old).await;
                assert!(is_registered(&host, &a).await.unwrap());
                assert!(context.workbenches.connected(&a));
                disconnect(&host, &context, &a, token, &new).await;
                assert!(!is_registered(&host, &a).await.unwrap());

                // A tab in the snapshot keeps the workbench.
                let (token, control) = ready(&host, &context, &a, None).await;
                put_snapshot(
                    &host,
                    a.clone(),
                    r#"{"version":4,"activeTabIndex":0,"tabs":[{"folderPath":"/tmp"}]}"#.to_owned(),
                )
                .await
                .unwrap();
                disconnect(&host, &context, &a, token, &control).await;
                assert!(is_registered(&host, &a).await.unwrap());
                close(&host, &context, a.clone(), None).await.unwrap();

                // A failed Session cleanup is sticky: every later transition
                // reports restart-required and durable state is kept.
                let b = "b".to_owned();
                let (token, control) = ready(&host, &context, &b, None).await;
                control.retire(Terminal::Disconnected);
                control.complete(Err(WireError::Internal {
                    message: "folder operation task failed: panic".to_owned(),
                }));
                detach(&host, &context, b.clone(), token.clone(), &control).await;
                let restart = Some(WireError::Internal {
                    message: CLEANUP_FAILED.to_owned(),
                });
                assert!(!context.workbenches.connected(&b));
                for _ in 0..2 {
                    assert_eq!(close(&host, &context, b.clone(), None).await.err(), restart);
                }
                assert!(is_registered(&host, &b).await.unwrap());
                assert_eq!(
                    attach(
                        &host,
                        &context,
                        b.clone(),
                        AttachMode::Resume {
                            controller_token: token
                        },
                        "browser".into()
                    )
                    .await
                    .err(),
                    Some(closing())
                );
            });
    }
}
