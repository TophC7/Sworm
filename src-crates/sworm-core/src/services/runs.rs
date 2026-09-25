//! Per-host run identity, reattachment, and optional completed-transcript policy.

use crate::errors::ApiError;
use crate::services::completed_runs::{CompletedRun, CompletedRunStore};
use crate::services::pty::{CompletedRunSink, PtyRunState, PtyService, PtySubscriber};
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Weak};
use sworm_protocol::rpc::RunStatus;
use sworm_protocol::session::SessionStartInfo;
use tracing::warn;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RunKind {
    Session { provider_id: String },
    Task { task_id: String },
}

struct RunRecord {
    folder: PathBuf,
    kind: RunKind,
    incarnation: u64,
    token: Option<Arc<Mutex<Option<String>>>>,
    archived: bool,
    _gate: Arc<Mutex<()>>,
}

#[derive(Default)]
struct RunState {
    records: HashMap<String, RunRecord>,
    gates: HashMap<String, Weak<Mutex<()>>>,
    next_incarnation: u64,
    completed: Option<Arc<CompletedRunStore>>,
    closed: bool,
}

#[derive(Clone, Default)]
pub(crate) struct RunCoordinator {
    state: Arc<Mutex<RunState>>,
    lifecycle: Arc<RwLock<()>>,
    // Closed labels remain tombstoned for this Host's lifetime: dropping one
    // would let a queued spawn_blocking start recreate an open owner gate.
    owners: Arc<Mutex<HashMap<String, Arc<OwnerGate>>>>,
}

#[derive(Default)]
struct OwnerGate {
    state: Mutex<OwnerState>,
    drained: Condvar,
}

#[derive(Default)]
struct OwnerState {
    closed: bool,
    active: usize,
}

pub(crate) struct OwnerActivity(Arc<OwnerGate>);

impl Drop for OwnerActivity {
    fn drop(&mut self) {
        let mut state = self.0.state.lock();
        state.active -= 1;
        if state.active == 0 {
            self.0.drained.notify_all();
        }
    }
}

pub(crate) struct RunGuard<'a> {
    coordinator: &'a RunCoordinator,
    gate: Option<Arc<Mutex<()>>>,
}

impl RunCoordinator {
    fn owner_gate(&self, owner: &str) -> Arc<OwnerGate> {
        let mut owners = self.owners.lock();
        Arc::clone(owners.entry(owner.to_owned()).or_default())
    }

    /// Register before startup, without holding any owner mutex during spawn.
    /// Teardown closes registration first, then waits for existing work to drain.
    pub(crate) fn owner_activity(
        &self,
        owner: Option<&str>,
    ) -> Result<Option<OwnerActivity>, ApiError> {
        let activity = if let Some(owner) = owner {
            let gate = self.owner_gate(owner);
            {
                let mut state = gate.state.lock();
                if state.closed {
                    return Err(ApiError::NotFound(format!(
                        "Run owner no longer exists: {owner}"
                    )));
                }
                state.active += 1;
            }
            Some(OwnerActivity(gate))
        } else {
            None
        };
        Ok(activity)
    }

    pub(crate) fn close_owner<T>(&self, owner: &str, action: impl FnOnce() -> T) -> T {
        let gate = self.owner_gate(owner);
        {
            let mut state = gate.state.lock();
            state.closed = true;
            while state.active != 0 {
                gate.drained.wait(&mut state);
            }
        }
        action()
    }
}

impl RunCoordinator {
    pub(crate) fn with_run<T>(
        &self,
        run_id: &str,
        action: impl FnOnce(&mut RunGuard<'_>) -> T,
    ) -> T {
        let _lifecycle = self.lifecycle.read();
        self.with_run_guard(run_id, action)
    }

    pub(crate) fn with_start<T>(
        &self,
        run_id: &str,
        action: impl FnOnce(&mut RunGuard<'_>) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let _lifecycle = self.lifecycle.read();
        if self.state.lock().closed {
            return Err(ApiError::NotFound("Host is shut down".into()));
        }
        self.with_run_guard(run_id, action)
    }

    fn with_run_guard<T>(&self, run_id: &str, action: impl FnOnce(&mut RunGuard<'_>) -> T) -> T {
        let gate = {
            let mut state = self.state.lock();
            match state.gates.get(run_id).and_then(Weak::upgrade) {
                Some(gate) => gate,
                None => {
                    let gate = Arc::new(Mutex::new(()));
                    state.gates.insert(run_id.to_owned(), Arc::downgrade(&gate));
                    gate
                }
            }
        };
        let _run = gate.lock();
        let result = action(&mut RunGuard {
            coordinator: self,
            gate: Some(Arc::clone(&gate)),
        });
        let mut state = self.state.lock();
        if Arc::strong_count(&gate) == 1 {
            state.gates.remove(run_id);
        }
        result
    }

    // Only whole-Host shutdown takes the lifecycle write lock.
    pub(crate) fn with_exclusive<T>(&self, action: impl FnOnce(&mut RunGuard<'_>) -> T) -> T {
        let _lifecycle = self.lifecycle.write();
        self.state.lock().closed = true;
        action(&mut RunGuard {
            coordinator: self,
            gate: None,
        })
    }

    pub(crate) fn release_unrestored_tasks(
        &self,
        owner: &str,
        restored: &std::collections::HashSet<String>,
        pty: &PtyService,
    ) -> Result<(), ApiError> {
        let owned = pty.owner_run_ids(owner, &HashSet::new());
        let candidates: Vec<_> = {
            let state = self.state.lock();
            owned
                .into_iter()
                .filter(|id| {
                    !restored.contains(id)
                        && state
                            .records
                            .get(id)
                            .is_some_and(|record| matches!(record.kind, RunKind::Task { .. }))
                })
                .collect()
        };
        let candidates = candidates
            .into_iter()
            .filter(|id| matches!(pty.run_state(id), Some(PtyRunState::Completed(_))));
        for id in candidates {
            self.with_run(&id, |runs| {
                let retained_task = self
                    .state
                    .lock()
                    .records
                    .get(&id)
                    .is_some_and(|record| matches!(record.kind, RunKind::Task { .. }));
                if retained_task
                    && matches!(pty.run_state(&id), Some(PtyRunState::Completed(_)))
                    && pty.ensure_owner(&id, Some(owner)).is_ok()
                {
                    runs.stop(
                        &id,
                        RunKind::Task {
                            task_id: String::new(),
                        },
                        pty,
                    )?;
                }
                Ok::<(), ApiError>(())
            })?;
        }
        Ok(())
    }

    /// Set once, before daemon runs start. Desktop hosts leave transcripts in memory.
    pub(crate) fn configure_completed_runs(&self, store: Arc<CompletedRunStore>) {
        let mut state = self.state.lock();
        assert!(
            state.records.is_empty() && state.completed.is_none(),
            "completed-run policy must be configured before starts"
        );
        state.completed = Some(store);
    }

    pub(crate) fn resume_token(&self, run_id: &str) -> Option<String> {
        let token = self
            .state
            .lock()
            .records
            .get(run_id)
            .and_then(|run| run.token.clone());
        token.and_then(|token| token.lock().clone())
    }

    pub(crate) fn completed_run(&self, run_id: &str) -> Result<Option<CompletedRun>, ApiError> {
        let store = self.state.lock().completed.clone();
        self.with_run(run_id, |_| {
            store.as_ref().map_or(Ok(None), |store| {
                store.get(run_id).map_err(ApiError::Internal)
            })
        })
    }

    pub(crate) fn status(&self, run_id: &str, pty: &PtyService) -> Result<RunStatus, ApiError> {
        let store = self.state.lock().completed.clone();
        self.with_run(run_id, |_| {
            let exited = match pty.run_state(run_id) {
                Some(PtyRunState::Live) => {
                    return Ok(RunStatus {
                        live: true,
                        exited: None,
                    })
                }
                Some(PtyRunState::Completed(code)) => Some(code),
                None => store.as_ref().map_or(Ok(None), |store| {
                    store.exit_status(run_id).map_err(ApiError::Internal)
                })?,
            };
            Ok(RunStatus {
                live: false,
                exited,
            })
        })
    }
}

impl RunGuard<'_> {
    pub(crate) fn clear(&mut self) {
        let mut state = self.coordinator.state.lock();
        state.records.clear();
        state.gates.retain(|_, gate| gate.strong_count() > 0);
    }

    fn existing(&self, run_id: &str, folder: &PathBuf, kind: &RunKind) -> Result<bool, ApiError> {
        match self.coordinator.state.lock().records.get(run_id) {
            Some(run) if &run.folder == folder && &run.kind == kind => Ok(true),
            Some(_) => Err(conflict(run_id)),
            None => Ok(false),
        }
    }

    fn known_state(&self, run_id: &str, pty: &PtyService) -> Result<bool, ApiError> {
        if pty.run_state(run_id).is_some() {
            return Ok(true);
        }
        let store = self.coordinator.state.lock().completed.clone();
        store.as_ref().map_or(Ok(false), |store| {
            store
                .exit_status(run_id)
                .map(|code| code.is_some())
                .map_err(ApiError::Internal)
        })
    }

    pub(crate) fn reuse_session(
        &self,
        run_id: &str,
        folder: &PathBuf,
        provider_id: &str,
        pty: &PtyService,
        subscriber: Option<&PtySubscriber>,
        owner: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<Option<SessionStartInfo>, ApiError> {
        if !self.reuse_run(
            run_id,
            folder,
            &RunKind::Session {
                provider_id: provider_id.to_owned(),
            },
            pty,
            subscriber,
            owner,
            cols,
            rows,
        )? {
            return Ok(None);
        }
        Ok(Some(SessionStartInfo {
            resumed: true,
            resume_token: self.coordinator.resume_token(run_id),
        }))
    }

    pub(crate) fn reuse_task(
        &self,
        run_id: &str,
        folder: &PathBuf,
        task_id: &str,
        pty: &PtyService,
        subscriber: Option<&PtySubscriber>,
        owner: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<bool, ApiError> {
        self.reuse_run(
            run_id,
            folder,
            &RunKind::Task {
                task_id: task_id.to_owned(),
            },
            pty,
            subscriber,
            owner,
            cols,
            rows,
        )
    }

    fn reuse_run(
        &self,
        run_id: &str,
        folder: &PathBuf,
        kind: &RunKind,
        pty: &PtyService,
        subscriber: Option<&PtySubscriber>,
        owner: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<bool, ApiError> {
        let existing = self.existing(run_id, folder, kind)?;
        let known = self.known_state(run_id, pty)?;
        if !existing && !known {
            return Ok(false);
        }
        if !existing && pty.run_state(run_id).is_some() {
            return Err(conflict(run_id));
        }
        if existing && !known {
            return Err(ApiError::NotFound(format!(
                "Run {run_id} has no retained transcript"
            )));
        }
        if let Some(subscriber) = subscriber {
            if pty.run_state(run_id).is_some() {
                pty.attach_same_owner(run_id, owner, subscriber.clone(), cols, rows)
                    .map_err(ApiError::Pty)?;
            } else {
                return Err(ApiError::NotFound(format!(
                    "Run {run_id} has no attachable PTY"
                )));
            }
        } else if existing && pty.run_state(run_id).is_some() {
            pty.ensure_owner(run_id, owner).map_err(ApiError::Pty)?;
        }
        Ok(true)
    }

    /// Reserve before spawning; a very short-lived child may exit while spawn returns.
    pub(crate) fn reserve(
        &mut self,
        run_id: &str,
        folder: PathBuf,
        kind: RunKind,
    ) -> (Option<Arc<Mutex<Option<String>>>>, Option<CompletedRunSink>) {
        let mut state = self.coordinator.state.lock();
        state.next_incarnation += 1;
        let incarnation = state.next_incarnation;
        let token = matches!(kind, RunKind::Session { .. }).then(|| Arc::new(Mutex::new(None)));
        state.records.insert(
            run_id.to_owned(),
            RunRecord {
                folder,
                kind,
                incarnation,
                token: token.clone(),
                archived: false,
                _gate: Arc::clone(
                    self.gate
                        .as_ref()
                        .expect("reservation requires a run guard"),
                ),
            },
        );
        let coordinator = self.coordinator.clone();
        let completed = state.completed.clone().map(|store| {
            Arc::new(move |run: CompletedRun| {
                let pruned = coordinator.with_run(&run.run_id, |_| {
                    if coordinator
                        .state
                        .lock()
                        .records
                        .get(&run.run_id)
                        .is_none_or(|record| record.incarnation != incarnation)
                    {
                        return Vec::new();
                    }
                    let mut pruned = Vec::new();
                    if let Err(error) = store.put(&run, |removed| {
                        coordinator
                            .state
                            .lock()
                            .records
                            .get_mut(&run.run_id)
                            .expect("incarnation checked under run guard")
                            .archived = true;
                        pruned.extend_from_slice(removed);
                    }) {
                        warn!("Failed to archive completed run {}: {error}", run.run_id);
                    }
                    pruned
                });
                // Pruning another ID's metadata must take that ID's gate, but
                // never while holding this run's gate or the archive write lock.
                for id in pruned {
                    coordinator.with_run(&id, |_| {
                        if store.exit_status(&id).is_ok_and(|status| status.is_none()) {
                            let mut state = coordinator.state.lock();
                            if state.records.get(&id).is_some_and(|record| record.archived) {
                                state.records.remove(&id);
                                if state
                                    .gates
                                    .get(&id)
                                    .is_some_and(|gate| gate.strong_count() == 0)
                                {
                                    state.gates.remove(&id);
                                }
                            }
                        }
                    });
                }
            }) as CompletedRunSink
        });
        (token, completed)
    }

    pub(crate) fn abort(&mut self, run_id: &str) {
        let mut state = self.coordinator.state.lock();
        state.records.remove(run_id);
        if state
            .gates
            .get(run_id)
            .is_some_and(|gate| gate.strong_count() == 0)
        {
            state.gates.remove(run_id);
        }
    }

    pub(crate) fn stop(
        &mut self,
        run_id: &str,
        kind: RunKind,
        pty: &PtyService,
    ) -> Result<(), ApiError> {
        if self
            .coordinator
            .state
            .lock()
            .records
            .get(run_id)
            .is_some_and(|record| {
                std::mem::discriminant(&record.kind) != std::mem::discriminant(&kind)
            })
        {
            return Err(conflict(run_id));
        }
        // Invalidate first: an exiting process must never re-create an explicitly deleted archive.
        self.abort(run_id);
        let result = match pty.kill(run_id) {
            Ok(()) => Ok(()),
            Err(error) if error.contains("No active PTY session") => Ok(()),
            Err(error) => Err(ApiError::Pty(error)),
        };
        let store = self.coordinator.state.lock().completed.clone();
        if let Some(store) = store {
            store.delete(run_id);
        }
        result
    }

    pub(crate) fn release(&mut self, run_id: &str) {
        self.abort(run_id);
        let store = self.coordinator.state.lock().completed.clone();
        if let Some(store) = store {
            store.delete(run_id);
        }
    }
}

fn conflict(run_id: &str) -> ApiError {
    ApiError::InvalidArgument(format!(
        "run id is already bound to different metadata: {run_id}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::HostEvent;
    use crate::host::Host;
    use std::fs;
    use std::time::{Duration, Instant};
    use sworm_protocol::pty::PtyEvent;

    struct Fixture {
        host: Arc<Host>,
        folder: PathBuf,
    }

    impl Fixture {
        fn new(command: &str) -> Self {
            let folder =
                std::env::temp_dir().join(format!("sworm-run-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(folder.join(".sworm")).unwrap();
            fs::write(folder.join(".sworm/tasks.jsonc"), serde_json::json!({
                "version": 1,
                "tasks": [{ "id": "once", "label": "Once", "command": command, "singleton": true }]
            }).to_string()).unwrap();
            let events: crate::events::EventSink<HostEvent> = Arc::new(|_| Ok(()));
            let host = Arc::new(Host::new(folder.join("data.db"), events).unwrap());
            Self { host, folder }
        }

        fn path(&self) -> String {
            self.folder.to_string_lossy().into_owned()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.host.shutdown();
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    fn subscriber() -> (PtySubscriber, Arc<Mutex<Vec<PtyEvent>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let events = Arc::clone(&seen);
        (
            PtySubscriber {
                output: Arc::new(|_| Ok(())),
                events: Arc::new(move |event| {
                    events.lock().push(event);
                    Ok(())
                }),
            },
            seen,
        )
    }

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for process output"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[tokio::test]
    async fn task_attach_only_and_reuse_never_execute_twice() {
        let fixture = Fixture::new("printf x >> count; sleep 30");
        let folder = fixture.path();
        let (first, first_events) = subscriber();
        assert!(matches!(
            fixture
                .host
                .tasks_start(
                    "run".into(),
                    folder.clone(),
                    "once".into(),
                    None,
                    80,
                    24,
                    Some(first),
                    Some("owner".into()),
                    true
                )
                .await,
            Err(ApiError::NotFound(_))
        ));
        assert!(!fixture.folder.join("count").exists());
        fixture
            .host
            .tasks_start(
                "run".into(),
                folder.clone(),
                "once".into(),
                None,
                80,
                24,
                None,
                Some("owner".into()),
                false,
            )
            .await
            .unwrap();
        wait_until(|| fs::read(fixture.folder.join("count")).is_ok_and(|bytes| bytes == b"x"));
        let (second, second_events) = subscriber();
        fixture
            .host
            .tasks_start(
                "run".into(),
                folder.clone(),
                "once".into(),
                None,
                90,
                30,
                Some(second),
                Some("owner".into()),
                true,
            )
            .await
            .unwrap();
        wait_until(|| {
            second_events
                .lock()
                .iter()
                .any(|event| matches!(event, PtyEvent::Started { .. }))
        });
        assert_eq!(fs::read(fixture.folder.join("count")).unwrap(), b"x");
        assert!(matches!(
            fixture
                .host
                .tasks_start(
                    "run".into(),
                    folder.clone(),
                    "different".into(),
                    None,
                    80,
                    24,
                    None,
                    Some("owner".into()),
                    false
                )
                .await,
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(matches!(
            fixture
                .host
                .tasks_start(
                    "run".into(),
                    folder,
                    "once".into(),
                    None,
                    80,
                    24,
                    None,
                    Some("other".into()),
                    true
                )
                .await,
            Err(ApiError::Pty(_))
        ));
        assert!(fixture.host.run_status("run").unwrap().live);
        assert!(first_events.lock().is_empty());
        fixture.host.tasks_stop("run".into()).await.unwrap();
        assert!(!fixture.host.run_status("run").unwrap().live);
    }

    #[tokio::test]
    async fn completed_local_task_replays_exit_without_respawn() {
        let fixture = Fixture::new("printf x >> count; printf retained-tail");
        let folder = fixture.path();
        fixture
            .host
            .tasks_start(
                "run".into(),
                folder.clone(),
                "once".into(),
                None,
                80,
                24,
                None,
                Some("owner".into()),
                false,
            )
            .await
            .unwrap();
        wait_until(|| fixture.host.run_status("run").unwrap().exited.is_some());
        let output = Arc::new(Mutex::new(Vec::new()));
        let seen_output = Arc::clone(&output);
        let events = Arc::new(Mutex::new(Vec::new()));
        let seen_events = Arc::clone(&events);
        let subscriber = PtySubscriber {
            output: Arc::new(move |bytes| {
                seen_output.lock().extend(bytes);
                Ok(())
            }),
            events: Arc::new(move |event| {
                seen_events.lock().push(event);
                Ok(())
            }),
        };
        fixture
            .host
            .tasks_start(
                "run".into(),
                folder,
                "once".into(),
                None,
                80,
                24,
                Some(subscriber),
                Some("owner".into()),
                true,
            )
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&output.lock()).contains("retained-tail"));
        assert!(events
            .lock()
            .iter()
            .any(|event| matches!(event, PtyEvent::Exit { .. })));
        assert_eq!(fs::read(fixture.folder.join("count")).unwrap(), b"x");
        fixture.host.tasks_stop("run".into()).await.unwrap();
        assert!(fixture.host.run_status("run").unwrap().exited.is_none());
    }
    #[tokio::test]
    async fn concurrent_task_starts_share_one_process() {
        let fixture = Fixture::new("printf x >> count; sleep 30");
        let folder = fixture.path();
        std::thread::scope(|scope| {
            let starts: Vec<_> = (0..2)
                .map(|_| {
                    let folder = folder.clone();
                    let host = &fixture.host;
                    scope.spawn(move || {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .build()
                            .unwrap();
                        runtime.block_on(host.tasks_start(
                            "race".into(),
                            folder,
                            "once".into(),
                            None,
                            80,
                            24,
                            None,
                            Some("owner".into()),
                            false,
                        ))
                    })
                })
                .collect();
            for start in starts {
                start.join().unwrap().unwrap();
            }
        });
        wait_until(|| fs::read(fixture.folder.join("count")).is_ok_and(|bytes| bytes == b"x"));
        assert_eq!(fs::read(fixture.folder.join("count")).unwrap(), b"x");
        fixture.host.tasks_stop("race".into()).await.unwrap();
    }

    #[tokio::test]
    async fn session_reattach_preserves_started_pid_and_rejects_other_owner() {
        let fixture = Fixture::new("true");
        let folder = fixture.path();
        let (first, first_events) = subscriber();
        fixture
            .host
            .session_start(
                "terminal-run".into(),
                folder.clone(),
                "terminal".into(),
                None,
                80,
                24,
                Some(first),
                Some("owner".into()),
            )
            .await
            .unwrap();
        wait_until(|| {
            first_events
                .lock()
                .iter()
                .any(|event| matches!(event, PtyEvent::Started { .. }))
        });
        let (second, second_events) = subscriber();
        let info = fixture
            .host
            .session_start(
                "terminal-run".into(),
                folder.clone(),
                "terminal".into(),
                None,
                90,
                30,
                Some(second),
                Some("owner".into()),
            )
            .await
            .unwrap();
        assert!(info.resumed);
        let first_pid = first_events
            .lock()
            .iter()
            .find_map(|event| match event {
                PtyEvent::Started { pid, .. } => Some(*pid),
                _ => None,
            })
            .unwrap();
        let second_pid = second_events
            .lock()
            .iter()
            .find_map(|event| match event {
                PtyEvent::Started { pid, .. } => Some(*pid),
                _ => None,
            })
            .unwrap();
        assert_eq!(first_pid, second_pid);
        assert!(matches!(
            fixture
                .host
                .session_start(
                    "terminal-run".into(),
                    folder,
                    "terminal".into(),
                    None,
                    80,
                    24,
                    None,
                    Some("other".into()),
                )
                .await,
            Err(ApiError::Pty(_))
        ));
        fixture
            .host
            .session_stop("terminal-run".into())
            .await
            .unwrap();
    }
    #[test]
    fn stalled_start_does_not_block_other_runs_and_same_run_stop_waits() {
        let fixture = Fixture::new("true");
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let subscriber = PtySubscriber {
            output: Arc::new(|_| Ok(())),
            events: Arc::new(move |event| {
                if matches!(event, PtyEvent::Started { .. }) {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .lock()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                }
                Ok(())
            }),
        };
        std::thread::scope(|scope| {
            let host = &fixture.host;
            let folder = fixture.path();
            let start = scope.spawn(move || {
                tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(host.session_start(
                        "slow".into(),
                        folder,
                        "terminal".into(),
                        None,
                        80,
                        24,
                        Some(subscriber),
                        None,
                    ))
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
            scope.spawn(move || {
                let result = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(host.session_stop("slow".into()));
                stopped_tx.send(result).unwrap();
            });
            let (other_tx, other_rx) = std::sync::mpsc::channel();
            let folder = fixture.path();
            scope.spawn(move || {
                let result = tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(host.session_start(
                        "other".into(),
                        folder,
                        "terminal".into(),
                        None,
                        80,
                        24,
                        None,
                        None,
                    ));
                other_tx.send(result).unwrap();
            });
            let other = other_rx.recv_timeout(Duration::from_secs(2));
            let stopped_early = stopped_rx.try_recv().is_ok();
            release_tx.send(()).unwrap();
            start.join().unwrap().unwrap();
            assert!(!stopped_early, "Stop must wait for the same run's startup");
            other.expect("unrelated startup blocked").unwrap();
            stopped_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap();
        });
        assert!(!fixture.host.run_status("slow").unwrap().live);
        assert!(fixture.host.run_status("other").unwrap().live);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn owner_close_waits_for_its_start_without_blocking_other_owner() {
        let fixture = Fixture::new("true");
        let folder = fixture.path();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let subscriber = PtySubscriber {
            output: Arc::new(|_| Ok(())),
            events: Arc::new(move |event| {
                if matches!(event, PtyEvent::Started { .. }) {
                    let _ = entered_tx.send(());
                    let _ = release_rx.lock().recv_timeout(Duration::from_secs(5));
                }
                Ok(())
            }),
        };
        let host = Arc::clone(&fixture.host);
        let first = tokio::spawn(async move {
            host.session_start(
                "first".into(),
                folder,
                "terminal".into(),
                None,
                80,
                24,
                Some(subscriber),
                Some("owner-a".into()),
            )
            .await
        });
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        let same_owner = tokio::time::timeout(Duration::from_secs(2), async {
            fixture
                .host
                .session_start(
                    "same-owner".into(),
                    fixture.path(),
                    "terminal".into(),
                    None,
                    80,
                    24,
                    None,
                    Some("owner-a".into()),
                )
                .await?;
            fixture.host.session_stop("same-owner".into()).await
        })
        .await;
        let host = Arc::clone(&fixture.host);
        let close =
            tokio::task::spawn_blocking(move || host.release_owner("owner-a", &Default::default()));
        let close_started = tokio::time::timeout(Duration::from_secs(5), async {
            while !fixture.host.runs.owner_gate("owner-a").state.lock().closed {
                tokio::task::yield_now().await;
            }
        })
        .await
        .is_ok();
        let other = tokio::time::timeout(Duration::from_secs(2), async {
            fixture
                .host
                .session_start(
                    "second".into(),
                    fixture.path(),
                    "terminal".into(),
                    None,
                    80,
                    24,
                    None,
                    Some("owner-b".into()),
                )
                .await?;
            fixture.host.session_stop("second".into()).await
        })
        .await;
        let close_waited = !close.is_finished();
        let _ = release_tx.send(());
        first.await.unwrap().unwrap();
        close.await.unwrap();
        assert!(close_started, "owner close did not begin");
        assert!(
            close_waited,
            "owner close must wait for its in-flight startup"
        );
        other.expect("unrelated owner blocked").unwrap();
        same_owner
            .expect("another run for same owner blocked")
            .unwrap();
        assert!(!fixture.host.run_status("first").unwrap().live);
        assert!(matches!(
            fixture
                .host
                .session_start(
                    "late".into(),
                    fixture.path(),
                    "terminal".into(),
                    None,
                    80,
                    24,
                    None,
                    Some("owner-a".into()),
                )
                .await,
            Err(ApiError::NotFound(_))
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restored_owner_reconciliation_ignores_other_owners_stalled_task() {
        let fixture = Fixture::new("true");
        fs::write(
            fixture.folder.join(".sworm/tasks.jsonc"),
            serde_json::json!({
                "version": 1,
                "tasks": [{ "id": "once", "label": "Once", "command": "true", "singleton": false }]
            })
            .to_string(),
        )
        .unwrap();
        fixture
            .host
            .tasks_start(
                "completed".into(),
                fixture.path(),
                "once".into(),
                None,
                80,
                24,
                None,
                Some("owner-b".into()),
                false,
            )
            .await
            .unwrap();
        wait_until(|| {
            fixture
                .host
                .run_status("completed")
                .unwrap()
                .exited
                .is_some()
        });

        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let subscriber = PtySubscriber {
            output: Arc::new(|_| Ok(())),
            events: Arc::new(move |event| {
                if matches!(event, PtyEvent::Started { .. }) {
                    let _ = entered_tx.send(());
                    let _ = release_rx.lock().recv_timeout(Duration::from_secs(5));
                }
                Ok(())
            }),
        };
        let host = Arc::clone(&fixture.host);
        let folder = fixture.path();
        let first = tokio::spawn(async move {
            host.tasks_start(
                "stalled".into(),
                folder,
                "once".into(),
                None,
                80,
                24,
                Some(subscriber),
                Some("owner-a".into()),
                false,
            )
            .await
        });
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(5)))
            .await
            .unwrap()
            .unwrap();
        let host = Arc::clone(&fixture.host);
        let mut ready = tokio::task::spawn_blocking(move || {
            host.release_unrestored_tasks("owner-b", &HashSet::new())
        });
        let independent = tokio::time::timeout(Duration::from_secs(2), &mut ready).await;
        let finished_before_release = independent.is_ok();
        let _ = release_tx.send(());
        first.await.unwrap().unwrap();
        match independent {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => ready.await.unwrap().unwrap(),
        }
        assert!(
            finished_before_release,
            "owner-b ready waited for owner-a task startup"
        );
        assert!(fixture
            .host
            .run_status("completed")
            .unwrap()
            .exited
            .is_none());
    }

    #[test]
    fn queued_start_cannot_spawn_after_shutdown() {
        let fixture = Fixture::new("printf x >> count");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(1)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            use std::future::Future;
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let blocker = tokio::task::spawn_blocking(move || {
                let _ = entered_tx.send(());
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let mut queued = Box::pin(fixture.host.tasks_start(
                "queued".into(),
                fixture.path(),
                "once".into(),
                None,
                80,
                24,
                None,
                Some("owner".into()),
                false,
            ));
            std::future::poll_fn(|cx| {
                assert!(queued.as_mut().poll(cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            fixture.host.shutdown();
            let _ = release_tx.send(());
            blocker.await.unwrap();
            assert!(matches!(queued.await, Err(ApiError::NotFound(_))));
            assert!(!fixture.folder.join("count").exists());
            assert!(fixture
                .host
                .tasks
                .register_singleton(fixture.folder.clone(), "once".into(), "probe".into(),)
                .is_ok());
            fixture.host.tasks.release_singleton_by_run_id("probe");
        });
    }

    #[tokio::test]
    async fn archive_pruning_retires_metadata_without_losing_retained_identity() {
        let fixture = Fixture::new("printf unexpected >> count");
        fixture
            .host
            .configure_completed_runs(Arc::new(CompletedRunStore::new(
                Arc::clone(&fixture.host.db),
                2,
                chrono::Duration::days(7),
            )));
        for id in ["old", "middle", "new"] {
            let completion = fixture.host.runs.with_run(id, |runs| {
                runs.reserve(
                    id,
                    fixture.folder.clone(),
                    RunKind::Task {
                        task_id: "once".into(),
                    },
                )
                .1
                .unwrap()
            });
            completion(CompletedRun {
                run_id: id.into(),
                exit_code: Some(0),
                output_start: 0,
                output: id.as_bytes().to_vec(),
                events: Vec::new(),
            });
        }
        assert!(fixture.host.completed_run("old").unwrap().is_none());
        assert_eq!(
            fixture.host.completed_run("new").unwrap().unwrap().output,
            b"new"
        );
        let state = fixture.host.runs.state.lock();
        assert_eq!(state.records.len(), 2);
        assert_eq!(state.gates.len(), 2);
        assert!(!state.records.contains_key("old"));
        drop(state);
        assert!(matches!(
            fixture
                .host
                .tasks_start(
                    "new".into(),
                    fixture.path(),
                    "wrong-task".into(),
                    None,
                    80,
                    24,
                    None,
                    None,
                    true,
                )
                .await,
            Err(ApiError::InvalidArgument(_))
        ));
        assert!(matches!(
            fixture
                .host
                .tasks_start(
                    "old".into(),
                    fixture.path(),
                    "once".into(),
                    None,
                    80,
                    24,
                    None,
                    None,
                    true,
                )
                .await,
            Err(ApiError::NotFound(_))
        ));
        assert!(!fixture.folder.join("count").exists());
    }

    #[tokio::test]
    async fn restored_view_releases_only_unrestored_completed_owned_tasks() {
        let fixture = Fixture::new("if test -e keepalive; then sleep 30; else printf tail; fi");
        for (id, owner) in [
            ("discard", "owner"),
            ("saved", "owner"),
            ("transfer", "owner"),
            ("foreign", "other"),
        ] {
            let (subscriber, _) = subscriber();
            fixture
                .host
                .tasks_start(
                    id.into(),
                    fixture.path(),
                    "once".into(),
                    None,
                    80,
                    24,
                    Some(subscriber),
                    Some(owner.into()),
                    false,
                )
                .await
                .unwrap();
            wait_until(|| fixture.host.run_status(id).unwrap().exited.is_some());
        }
        fixture.host.pty.pause_owned("transfer", "owner").unwrap();
        fs::write(fixture.folder.join("keepalive"), "").unwrap();
        fixture
            .host
            .tasks_start(
                "live".into(),
                fixture.path(),
                "once".into(),
                None,
                80,
                24,
                None,
                Some("owner".into()),
                false,
            )
            .await
            .unwrap();
        fixture
            .host
            .release_unrestored_tasks("owner", &["saved".into()].into())
            .unwrap();
        assert!(fixture.host.run_status("discard").unwrap().exited.is_none());
        for id in ["saved", "transfer", "foreign"] {
            assert!(
                fixture.host.run_status(id).unwrap().exited.is_some(),
                "{id} must remain retained"
            );
        }
        assert!(fixture.host.run_status("live").unwrap().live);
        let state = fixture.host.runs.state.lock();
        assert!(!state.records.contains_key("discard"));
        assert!(!state.gates.contains_key("discard"));
    }

    #[test]
    fn pruning_waits_for_the_evicted_runs_gate() {
        let fixture = Fixture::new("true");
        let store = Arc::new(CompletedRunStore::new(
            Arc::clone(&fixture.host.db),
            1,
            chrono::Duration::days(7),
        ));
        fixture.host.configure_completed_runs(Arc::clone(&store));
        let old = fixture.host.runs.with_run("old", |runs| {
            runs.reserve(
                "old",
                fixture.folder.clone(),
                RunKind::Task {
                    task_id: "once".into(),
                },
            )
            .1
            .unwrap()
        });
        old(CompletedRun {
            run_id: "old".into(),
            exit_code: Some(0),
            output_start: 0,
            output: Vec::new(),
            events: Vec::new(),
        });
        std::thread::sleep(Duration::from_millis(2));
        let new = fixture.host.runs.with_run("new", |runs| {
            runs.reserve(
                "new",
                fixture.folder.clone(),
                RunKind::Task {
                    task_id: "once".into(),
                },
            )
            .1
            .unwrap()
        });
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let host = &fixture.host;
            let held = scope.spawn(move || {
                host.runs.with_run("old", |_| {
                    let _ = entered_tx.send(());
                    let _ = release_rx.recv_timeout(Duration::from_secs(5));
                })
            });
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let archive = scope.spawn(move || {
                new(CompletedRun {
                    run_id: "new".into(),
                    exit_code: Some(0),
                    output_start: 0,
                    output: Vec::new(),
                    events: Vec::new(),
                })
            });
            let deadline = Instant::now() + Duration::from_secs(4);
            let mut pruned = false;
            while Instant::now() < deadline {
                if store.exit_status("old").unwrap().is_none() {
                    pruned = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let metadata_held = fixture.host.runs.state.lock().records.contains_key("old");
            let _ = release_tx.send(());
            held.join().unwrap();
            archive.join().unwrap();
            assert!(pruned, "new archive never pruned old run");
            assert!(metadata_held, "pruning bypassed the old run's gate");
        });
        assert!(!fixture.host.runs.state.lock().records.contains_key("old"));
    }

    #[test]
    fn archive_io_does_not_hold_coordinator_metadata_lock() {
        let fixture = Fixture::new("true");
        fixture
            .host
            .configure_completed_runs(Arc::new(CompletedRunStore::new(
                Arc::clone(&fixture.host.db),
                2,
                chrono::Duration::days(7),
            )));
        let completion = fixture.host.runs.with_run("archive", |runs| {
            runs.reserve(
                "archive",
                fixture.folder.clone(),
                RunKind::Task {
                    task_id: "once".into(),
                },
            )
            .1
            .unwrap()
        });
        let gate = fixture.host.runs.state.lock().records["archive"]
            ._gate
            .clone();
        let db = fixture.host.db.write();
        std::thread::scope(|scope| {
            let archive = scope.spawn(move || {
                completion(CompletedRun {
                    run_id: "archive".into(),
                    exit_code: Some(0),
                    output_start: 0,
                    output: b"tail".to_vec(),
                    events: Vec::new(),
                })
            });
            wait_until(|| gate.is_locked());
            wait_until(|| fixture.host.runs.state.try_lock().is_some());
            drop(db);
            archive.join().unwrap();
        });
        assert_eq!(
            fixture
                .host
                .completed_run("archive")
                .unwrap()
                .unwrap()
                .output,
            b"tail"
        );
    }

    #[tokio::test]
    async fn explicit_stop_fences_late_archive_publication() {
        let fixture = Fixture::new("true");
        fixture
            .host
            .configure_completed_runs(Arc::new(CompletedRunStore::new(
                Arc::clone(&fixture.host.db),
                200,
                chrono::Duration::days(7),
            )));
        let completion = fixture.host.runs.with_run("closed", |runs| {
            let (_, completion) = runs.reserve(
                "closed",
                fixture.folder.clone(),
                RunKind::Task {
                    task_id: "once".into(),
                },
            );
            completion.unwrap()
        });
        fixture.host.tasks_stop("closed".into()).await.unwrap();
        fixture.host.runs.with_run("closed", |runs| {
            runs.reserve(
                "closed",
                fixture.folder.clone(),
                RunKind::Task {
                    task_id: "once".into(),
                },
            );
        });
        completion(CompletedRun {
            run_id: "closed".into(),
            exit_code: Some(0),
            output_start: 0,
            output: b"should-not-reappear".to_vec(),
            events: Vec::new(),
        });
        assert!(fixture.host.completed_run("closed").unwrap().is_none());
    }
}
