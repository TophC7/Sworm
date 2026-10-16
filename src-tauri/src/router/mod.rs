mod file_streams;
mod leases;
mod ops;
mod pending_stops;
mod remote_lsp;
mod remote_runs;
pub mod remotes;
mod target;
mod worktrees;

use crate::host_events::DesktopEvent;
use file_streams::FileStreams;
use leases::Leases;
use pending_stops::{PendingStop, PendingStops};
pub use remotes::RemoteStatus;
use remotes::{RemoteSlot, SettingsCache};
use target::remote_error;
pub use target::{reject_remote, Target};

use parking_lot::Mutex;
use remote_runs::{RemoteRunKind, RemoteRunService};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};
use sworm_core::{
    errors::ApiError,
    events::{EventSink, HostEvent},
    Host,
};
use sworm_protocol::rpc::{Reply, Request};
use sworm_remote::{client::client_endpoint, Identity, RemoteError};
use tokio::sync::{Mutex as AsyncMutex, OnceCell};
pub(crate) struct RouterInner {
    pub(crate) host: Arc<Host>,
    events: EventSink<DesktopEvent>,
    remotes: Mutex<HashMap<String, Arc<RemoteSlot>>>,
    /// Bound on first remote use: QUIC needs a live runtime; setup has none.
    endpoint: LazyLock<quinn::Endpoint>,
    settings: AsyncMutex<SettingsCache>,
    identity: OnceCell<Arc<Identity>>,
    remote_management: AsyncMutex<()>,
    leases: Mutex<Leases>,
    transitions: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    pending_stops: PendingStops,
    file_streams: FileStreams,
    pub(crate) remote_runs: RemoteRunService,
    pub(crate) remote_lsp: remote_lsp::RemoteLspService,
}

#[derive(Clone)]
pub struct WorkspaceRouter {
    inner: Arc<RouterInner>,
}

impl WorkspaceRouter {
    pub fn new(host: Arc<Host>) -> Self {
        Self::with_events(host, Arc::new(|_| Ok(())))
    }

    /// Host ops are synchronous and may block on git, the filesystem or child
    /// processes; every local call leaves the async runtime here.
    async fn local<T: Send + 'static>(
        &self,
        op: impl FnOnce(&Arc<Host>) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        self.inner.local(op).await
    }

    pub fn with_events(host: Arc<Host>, events: EventSink<DesktopEvent>) -> Self {
        let pending_stops = PendingStops::load(&host);
        Self {
            inner: Arc::new(RouterInner {
                host,
                endpoint: LazyLock::new(client_endpoint),
                remotes: Mutex::new(HashMap::new()),
                settings: AsyncMutex::new(SettingsCache {
                    generation: u64::MAX,
                    remotes: HashMap::new(),
                }),
                identity: OnceCell::new(),
                events,
                leases: Mutex::new(Leases::default()),
                transitions: Mutex::new(HashMap::new()),
                remote_runs: RemoteRunService::new(),
                remote_lsp: remote_lsp::RemoteLspService::new(),
                pending_stops,
                remote_management: AsyncMutex::new(()),
                file_streams: FileStreams::new(),
            }),
        }
    }

    /// Resume stops that never reached their daemon: an outage during a tab
    /// close, or a crash before the retry landed.
    pub fn retry_pending_stops(&self) {
        self.inner.drain_pending_stops();
    }

    pub async fn run_write(&self, run_id: String, data: Vec<u8>) -> Result<(), ApiError> {
        self.local(move |host| host.run_write(run_id, data)).await
    }

    pub async fn run_resize(&self, run_id: String, cols: u16, rows: u16) -> Result<(), ApiError> {
        self.local(move |host| host.run_resize(run_id, cols, rows))
            .await
    }

    /// Ordered client messages: a remote session's stream, a local server's
    /// stdin. Not a table op because ordering rules out a fresh RPC stream.
    pub async fn lsp_send(&self, session_id: String, message_json: String) -> Result<(), ApiError> {
        if let Some(result) = self.inner.remote_lsp.send(&session_id, &message_json) {
            return result;
        }
        self.local(move |host| host.lsp_send(session_id, message_json))
            .await
    }

    pub fn server_for_run(&self, run_id: &str) -> Option<String> {
        self.inner.remote_runs.server_for(run_id)
    }

    /// Force the same QUIC close path used by a real network loss.
    #[doc(hidden)]
    pub async fn close_remote_for_test(&self, server: &str) -> bool {
        let slot = self.inner.remotes.lock().get(server).cloned();
        let Some(slot) = slot else {
            return false;
        };
        let cached = slot.cached.lock().await;
        let Some(cached) = cached.as_ref() else {
            return false;
        };
        cached.client.close();
        true
    }

    /// Stops still waiting for their daemon to acknowledge them.
    #[doc(hidden)]
    pub fn pending_stops_for_test(&self) -> HashMap<String, String> {
        self.inner
            .pending_stops
            .map
            .lock()
            .values()
            .map(|stop| (stop.run_id.clone(), stop.server.clone()))
            .collect()
    }

    #[doc(hidden)]
    pub fn queue_stop_for_test(&self, server: &str, run_id: &str) {
        self.inner.pending_stops.insert(
            &self.inner.host,
            PendingStop {
                server: server.to_owned(),
                workbench: "test-workbench".into(),
                run_id: run_id.to_owned(),
                kind: RemoteRunKind::Session,
                controller_token: String::new(),
            },
        );
    }

    /// Only an open tab declares durable ownership. Folder discovery and Home
    /// metadata reads must never create reconnect declarations.
    pub async fn claim_folder(&self, folder_path: String) -> Result<(), ApiError> {
        let Target::Remote { server, path } = Target::parse(&folder_path)? else {
            return Ok(());
        };
        let server = server.to_owned();
        let path = Target::normalize_remote_path(path)?;
        let slot = self.inner.slot(&server);
        let router = self.clone();
        tokio::spawn(async move {
            let _restore = slot.claim_restore.lock().await;
            router.inner.remember_claim(&server, &path, None, false);
            router
                .call_reply(&server, Request::FolderClaim { folder_path: path })
                .await?
                .folder_claim()
                .map_err(ApiError::from)
        })
        .await?
    }

    /// Window teardown retains its nonblocking policy. Checked worktree closure
    /// uses the awaited path below and propagates every daemon release failure.
    pub fn release_folder(&self, folder_path: &str) {
        let router = self.clone();
        let folder_path = folder_path.to_owned();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = router.release_folder_strict(folder_path).await {
                tracing::warn!(%error, "remote folder release failed");
            }
        });
    }

    pub async fn release_folder_strict(&self, folder_path: String) -> Result<(), ApiError> {
        let Target::Remote { server, path } = Target::parse(&folder_path)? else {
            return Ok(());
        };
        let server = server.to_owned();
        let path = Target::normalize_remote_path(path)?;
        let slot = self.inner.remotes.lock().get(&server).cloned();
        let Some(slot) = slot else {
            return Ok(());
        };
        let router = self.clone();
        tokio::spawn(async move {
            let _restore = slot.claim_restore.lock().await;
            slot.claims.lock().retain(|key, _| {
                key != &path
                    && Target::normalize_remote_path(key).ok().as_deref() != Some(path.as_str())
            });
            let client = router.inner.client(&server).await?;
            client
                .call(&Request::FolderRelease { folder_path: path })
                .await
                .map_err(|error| remote_error(&server, error))?
                .folder_release()
                .map_err(ApiError::from)?;
            router.inner.stop_events_if_idle(&server);
            Ok(())
        })
        .await?
    }

    async fn call_reply(&self, server: &str, request: Request) -> Result<Reply, ApiError> {
        self.inner.call_reply(server, request).await
    }

    /// Release streams and workbench leases owned by a closing window.
    pub fn release_window(&self, owner: &str) {
        self.inner.file_streams.release_owner(owner);
        self.inner.remote_lsp.release_owner(owner);
        let leases: Vec<_> = self
            .inner
            .leases
            .lock()
            .0
            .iter()
            .filter(|((window, _), _)| window == owner)
            .map(|((_, server), lease)| (server.clone(), lease.clone()))
            .collect();
        for (server, lease) in leases {
            let router = self.clone();
            let owner = owner.to_owned();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = router
                    .workbench_detach(&owner, &server, lease.id, lease.attachment_id)
                    .await
                {
                    tracing::warn!(%server, %error, "workbench detach on window close failed");
                }
            });
        }
    }

    pub async fn remote_runs_release(
        &self,
        owner: &str,
        run_ids: &[String],
    ) -> Result<(), ApiError> {
        let admission = Arc::clone(&self.inner.host.admission).read_owned().await;
        let inner = Arc::clone(&self.inner);
        let owner = owner.to_owned();
        let run_ids = run_ids.to_owned();
        tokio::task::spawn_blocking(move || {
            let _admission = admission;
            inner.remote_runs.release(&inner.host, &owner, &run_ids)
        })
        .await?
    }

    /// A remote mutation runs inside the daemon's `Host`, whose `FileMoved`
    /// and `FileDeleted` events are local bookkeeping the events stream never
    /// carries. Re-emitting them here against workspace URIs keeps window
    /// claims and the editor's path tracking identical to a local folder.
    fn emit(&self, event: HostEvent) -> Result<(), ApiError> {
        (self.inner.events)(DesktopEvent::Host(event)).map_err(ApiError::Internal)
    }

    /// This desktop's identity fingerprint, listed by paired servers.
    pub async fn client_fingerprint(&self) -> Result<String, ApiError> {
        Ok(self
            .inner
            .client_identity()
            .await?
            .fingerprint()
            .to_string())
    }
}

impl RouterInner {
    /// Host ops are synchronous and may block on git, the filesystem or child
    /// processes; every local call leaves the async runtime here.
    async fn local<T: Send + 'static>(
        &self,
        op: impl FnOnce(&Arc<Host>) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let admission = Arc::clone(&self.host.admission).read_owned().await;
        let host = Arc::clone(&self.host);
        tokio::task::spawn_blocking(move || {
            let _admission = admission;
            op(&host)
        })
        .await?
    }

    pub(crate) fn emit_run_status(&self, run_id: &str, state: &str) {
        let _ = (self.events)(DesktopEvent::RemoteRunStatus {
            run_id: run_id.into(),
            state: state.into(),
        });
    }

    async fn call_reply(&self, server: &str, request: Request) -> Result<Reply, ApiError> {
        self.call_reply_for(server, None, request).await
    }

    async fn call_reply_for(
        &self,
        server: &str,
        workbench: Option<&str>,
        request: Request,
    ) -> Result<Reply, ApiError> {
        let client = self.client(server).await?;
        let result = match workbench {
            Some(id) => client.call_workbench(id, &request).await,
            None => client.call(&request).await,
        };
        match result {
            Ok(value) => return Ok(value),
            Err(error) if matches!(&error, RemoteError::Connection(_)) || client.is_closed() => {
                self.evict(server, &client).await
            }
            Err(error) => return Err(remote_error(server, error)),
        }

        let client = self.client(server).await?;
        let result = match workbench {
            Some(id) => client.call_workbench(id, &request).await,
            None => client.call(&request).await,
        };
        if matches!(&result, Err(RemoteError::Connection(_))) || client.is_closed() {
            self.evict(server, &client).await;
        }
        result.map_err(|error| remote_error(server, error))
    }
}
