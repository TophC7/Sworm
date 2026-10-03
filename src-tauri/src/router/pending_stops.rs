use super::{
    leases::Leases, remote_runs::RemoteRunKind, remotes::Backoff, target::not_controller,
    RouterInner,
};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Weak},
};
use sworm_core::{errors::ApiError, services::app_state_kv::AppStateKvService, Host};

const PENDING_STOPS_KEY: &str = "remote:pendingStops";

/// A stop the daemon never acknowledged. It outlives the tab, the window, and
/// the app itself: a closed tab must never strand a process on a server.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct PendingStop {
    pub(super) server: String,
    pub(super) workbench: String,
    pub(super) run_id: String,
    pub(super) kind: RemoteRunKind,
    #[serde(default)]
    pub(super) controller_token: String,
}

pub(super) struct PendingStops {
    pub(super) map: Mutex<HashMap<String, PendingStop>>,
    running: Mutex<HashSet<String>>,
}

impl PendingStops {
    /// Load stops left unacknowledged by an earlier run of the app.
    pub(super) fn load(host: &Host) -> Self {
        let stored = {
            let db = host.db.read();
            AppStateKvService::get(db.conn(), PENDING_STOPS_KEY)
        };
        let entries: Vec<PendingStop> = match stored {
            Ok(Some(json)) => serde_json::from_str(&json).unwrap_or_else(|error| {
                tracing::error!(%error, "failed to decode pending remote stops");
                Vec::new()
            }),
            Ok(None) => Vec::new(),
            Err(error) => {
                tracing::error!(%error, "failed to read pending remote stops");
                Vec::new()
            }
        };
        Self {
            map: Mutex::new(
                entries
                    .into_iter()
                    .map(|entry| (entry.run_id.clone(), entry))
                    .collect(),
            ),
            running: Mutex::new(HashSet::new()),
        }
    }

    pub(super) fn persist(&self, host: &Host, pending: &HashMap<String, PendingStop>) {
        let entries: Vec<&PendingStop> = pending.values().collect();
        let json = match serde_json::to_string(&entries) {
            Ok(json) => json,
            Err(error) => {
                tracing::error!(%error, "failed to encode pending remote stops");
                return;
            }
        };
        let db = host.db.write();
        if let Err(error) = AppStateKvService::put(db.conn(), PENDING_STOPS_KEY, &json) {
            tracing::error!(%error, "failed to persist pending remote stops");
        }
    }

    /// Forget `stop` unless a rename re-pointed it at another server while it
    /// was in flight: the old name failing says nothing about the run.
    pub(super) fn forget(&self, host: &Host, stop: &PendingStop) {
        let mut pending = self.map.lock();
        if pending.get(&stop.run_id).is_some_and(|current| {
            current.server == stop.server && current.controller_token == stop.controller_token
        }) {
            pending.remove(&stop.run_id);
            self.persist(host, &pending);
        }
    }

    pub(super) fn drain(&self, router: &Arc<RouterInner>, leases: &Leases) {
        let servers: HashSet<String> = self
            .map
            .lock()
            .values()
            .filter(|stop| leases.current(stop))
            .map(|stop| stop.server.clone())
            .collect();
        let mut running = self.running.lock();
        for server in servers {
            if !running.insert(server.clone()) {
                continue;
            }
            let router = Arc::downgrade(router);
            // Tauri's runtime: the first drain runs from setup, outside tokio.
            tauri::async_runtime::spawn(async move {
                retry_server_stops(router, server).await;
            });
        }
    }

    pub(super) fn retain(&self, host: &Host, mut keep: impl FnMut(&PendingStop) -> bool) {
        let mut pending = self.map.lock();
        let before = pending.len();
        pending.retain(|_, stop| keep(stop));
        if pending.len() != before {
            self.persist(host, &pending);
        }
    }

    pub(super) fn insert(&self, host: &Host, stop: PendingStop) {
        let mut pending = self.map.lock();
        pending.insert(stop.run_id.clone(), stop);
        self.persist(host, &pending);
    }

    pub(super) fn on_server(&self, server: &str, leases: &Leases) -> Vec<PendingStop> {
        self.map
            .lock()
            .values()
            .filter(|stop| stop.server == server && leases.current(stop))
            .cloned()
            .collect()
    }
}

impl RouterInner {
    pub(crate) async fn stop_backend(
        self: &Arc<Self>,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let (server, workbench) = self
            .remote_runs
            .target_for(run_id)
            .ok_or_else(|| ApiError::NotFound(format!("Unknown remote run `{run_id}`")))?;
        self.stop_backend_on(&server, &workbench, run_id, kind)
            .await
    }

    pub(super) async fn stop_backend_on(
        self: &Arc<Self>,
        server: &str,
        workbench: &str,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let transition = self.transition(server);
        let _guard = transition.lock().await;
        let result = async {
            self.reconcile(server).await?;
            if self.leases.lock().holder(server, workbench).is_none() {
                return Err(not_controller(workbench));
            }
            self.stop_backend_locked(server, workbench, run_id, kind)
                .await
        }
        .await;
        if let Err(error) = &result {
            // Capture authority before releasing the transition gate. A later
            // takeover must not relabel this old stop with its new token.
            self.remember_failed_stop(server, workbench, run_id, kind, error);
        }
        result
    }

    pub(super) async fn stop_backend_locked(
        &self,
        server: &str,
        workbench: &str,
        run_id: &str,
        kind: RemoteRunKind,
    ) -> Result<(), ApiError> {
        let request = kind.stop_request(run_id.to_owned());
        let reply = self
            .call_reply_for(server, Some(workbench), request)
            .await?;
        match kind {
            RemoteRunKind::Session => reply.session_stop(),
            RemoteRunKind::Task => reply.tasks_stop(),
        }
        .map_err(ApiError::from)
    }

    /// Remember a stop the daemon never acknowledged, then keep retrying it.
    pub(crate) fn remember_failed_stop(
        self: &Arc<Self>,
        server: &str,
        workbench: &str,
        run_id: &str,
        kind: RemoteRunKind,
        error: &ApiError,
    ) {
        if matches!(error, ApiError::NotFound(_)) {
            // Neither the run nor the server exists any more: nothing to kill.
            return;
        }
        let leases = self.leases.lock();
        let Some(lease) = leases.holder(server, workbench) else {
            return;
        };
        self.pending_stops.insert(
            &self.host,
            PendingStop {
                workbench: workbench.to_owned(),
                server: server.to_owned(),
                run_id: run_id.to_owned(),
                kind,
                controller_token: lease.controller_token.clone(),
            },
        );
        drop(leases);
        self.drain_pending_stops();
    }

    pub(super) fn drain_pending_stops(self: &Arc<Self>) {
        let leases = self.leases.lock();
        self.pending_stops.drain(self, &leases);
    }
}

/// Each server retries sequentially with its own backoff; an unavailable
/// daemon cannot delay stops headed to another server.
async fn retry_server_stops(router: Weak<RouterInner>, server: String) {
    let mut backoff = Backoff::default();
    loop {
        let Some(inner) = router.upgrade() else {
            return;
        };
        let entries: Vec<PendingStop> = {
            let leases = inner.leases.lock();
            inner.pending_stops.on_server(&server, &leases)
        };
        if entries.is_empty() {
            inner.pending_stops.running.lock().remove(&server);
            inner.drain_pending_stops();
            return;
        }
        let mut failed = false;
        for entry in entries {
            let transition = inner.transition(&server);
            let _guard = transition.lock().await;
            if let Err(error) = inner.reconcile(&server).await {
                failed = true;
                tracing::warn!(%server, %error, "pending stop reconciliation failed");
                continue;
            }
            if !inner.leases.lock().current(&entry) {
                inner.pending_stops.forget(&inner.host, &entry);
                continue;
            }
            match inner
                .stop_backend_locked(&server, &entry.workbench, &entry.run_id, entry.kind)
                .await
            {
                Ok(()) | Err(ApiError::NotFound(_)) => {
                    inner.pending_stops.forget(&inner.host, &entry)
                }
                Err(error) => {
                    failed = true;
                    tracing::warn!(%server, run_id = entry.run_id, %error, "retrying remote stop");
                }
            }
        }
        drop(inner);
        if failed {
            backoff.wait().await;
        } else {
            backoff.reset();
        }
    }
}
