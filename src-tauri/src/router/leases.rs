use super::{
    pending_stops::PendingStop,
    target::{not_controller, remote_error},
    RouterInner, WorkspaceRouter,
};
use crate::host_events::DesktopEvent;
use std::{collections::HashMap, sync::Arc};
use sworm_core::errors::ApiError;
use sworm_protocol::rpc::{AttachMode, Request, WorkbenchAttached, WorkbenchInfo};
use sworm_remote::{RemoteClient, RemoteError};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone, Debug)]
pub(super) struct WorkbenchLease {
    pub(super) id: String,
    pub(super) attachment_id: String,
    pub(super) controller_token: String,
}

#[derive(Default)]
pub(super) struct Leases(pub(super) HashMap<(String, String), WorkbenchLease>);

impl Leases {
    pub(super) fn owns(&self, owner: &str, server: &str, id: &str, attachment: &str) -> bool {
        self.0
            .get(&(owner.to_owned(), server.to_owned()))
            .is_some_and(|lease| lease.id == id && lease.attachment_id == attachment)
    }

    pub(super) fn holder(&self, server: &str, id: &str) -> Option<&WorkbenchLease> {
        self.0
            .iter()
            .find_map(|((_, remote), lease)| (remote == server && lease.id == id).then_some(lease))
    }

    pub(super) fn on_server(&self, server: &str) -> bool {
        self.0.keys().any(|(_, remote)| remote == server)
    }

    pub(super) fn current(&self, stop: &PendingStop) -> bool {
        self.0.iter().any(|((_, server), lease)| {
            server == &stop.server
                && lease.id == stop.workbench
                && lease.controller_token == stop.controller_token
        })
    }
}

impl WorkspaceRouter {
    pub async fn workbench_list(&self, server: &str) -> Result<Vec<WorkbenchInfo>, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.inner.reconcile(server).await
    }

    pub async fn workbench_list_for_owner(
        &self,
        owner: &str,
        server: &str,
    ) -> Result<Vec<WorkbenchInfo>, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        let mut rows = self.inner.reconcile(server).await?;
        let leases = self.inner.leases.lock();
        let lease = leases.0.get(&(owner.to_owned(), server.to_owned()));
        for row in &mut rows {
            row.yours &= lease.is_some_and(|lease| lease.id == row.id);
        }
        Ok(rows)
    }

    pub async fn workbench_close(&self, server: &str, id: String) -> Result<(), ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.call_reply(server, Request::WorkbenchClose { id: id.clone() })
            .await?
            .workbench_close()
            .map_err(ApiError::from)?;
        self.inner.clear_workbench(server, &id);
        Ok(())
    }

    pub async fn workbench_attach(
        &self,
        owner: &str,
        server: &str,
        id: String,
        mode: AttachMode,
        attachment_id: String,
    ) -> Result<Option<WorkbenchAttached>, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.inner.reconcile(server).await?;
        if self.workbench_owner(server, &id, owner).is_some() {
            return Ok(None);
        }
        let previous = self
            .inner
            .leases
            .lock()
            .0
            .get(&(owner.to_owned(), server.to_owned()))
            .cloned();
        if let Some(previous) = previous.as_ref().filter(|previous| previous.id != id) {
            self.detach_locked(
                owner,
                server,
                previous.id.clone(),
                previous.attachment_id.clone(),
            )
            .await?;
        }
        let client = self.inner.client(server).await?;
        let request = Request::WorkbenchAttach {
            id: id.clone(),
            mode,
            attachment_id: attachment_id.clone(),
            client: gethostname::gethostname().to_string_lossy().into_owned(),
        };
        let attached = match client.call(&request).await {
            Ok(reply) => reply.workbench_attach().map_err(ApiError::from),
            Err(error)
                if !client.is_closed()
                    && matches!(error, RemoteError::Timeout(_) | RemoteError::Transport(_)) =>
            {
                // A lost response says nothing about admission. Recover only
                // this exact operation on this connection; never replay it.
                self.inner
                    .recover_workbench(server, &client, id.clone(), attachment_id)
                    .await
                    .and_then(|result| {
                        result.ok_or_else(|| {
                            ApiError::Remote(format!("{server}: attach result no longer available"))
                        })
                    })
            }
            Err(error) => {
                if client.is_closed() || matches!(error, RemoteError::Connection(_)) {
                    self.inner.evict(server, &client).await;
                }
                Err(remote_error(server, error))
            }
        };
        if let Ok(WorkbenchAttached::Ready {
            controller_token,
            attachment_id,
            ..
        }) = &attached
        {
            self.inner.leases.lock().0.insert(
                (owner.to_owned(), server.to_owned()),
                WorkbenchLease {
                    id: id.clone(),
                    attachment_id: attachment_id.clone(),
                    controller_token: controller_token.clone(),
                },
            );
            self.inner.drain_pending_stops();
            let slot = self.inner.slot(server);
            self.inner.ensure_events(server, &slot);
        } else if matches!(
            &attached,
            Ok(WorkbenchAttached::Busy { .. } | WorkbenchAttached::Revoked { .. })
        ) {
            if let Some(previous) = previous.filter(|lease| lease.id == id) {
                self.inner
                    .clear_lease(owner, server, &id, &previous.attachment_id);
            }
        }
        if let Err(error) = self.inner.reconcile(server).await {
            tracing::warn!(%server, %error, "workbench reconciliation after attach failed");
        }
        attached.map(Some)
    }

    pub fn workbench_owned(
        &self,
        owner: &str,
        server: &str,
        id: &str,
        attachment_id: &str,
    ) -> bool {
        self.inner
            .leases
            .lock()
            .owns(owner, server, id, attachment_id)
    }
    pub(crate) fn workbench_owner(&self, server: &str, id: &str, owner: &str) -> Option<String> {
        self.inner
            .leases
            .lock()
            .0
            .iter()
            .find(|((window, remote), lease)| {
                window.as_str() != owner && remote.as_str() == server && lease.id == id
            })
            .map(|((window, _), _)| window.clone())
    }

    pub async fn workbench_detach(
        &self,
        owner: &str,
        server: &str,
        id: String,
        attachment_id: String,
    ) -> Result<(), ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.detach_locked(owner, server, id, attachment_id).await
    }

    pub(super) async fn detach_locked(
        &self,
        owner: &str,
        server: &str,
        id: String,
        attachment_id: String,
    ) -> Result<(), ApiError> {
        if !self
            .inner
            .leases
            .lock()
            .owns(owner, server, &id, &attachment_id)
        {
            return Ok(());
        }
        self.call_reply(
            server,
            Request::WorkbenchDetach {
                id: id.clone(),
                attachment_id: attachment_id.clone(),
            },
        )
        .await?
        .workbench_detach()
        .map_err(ApiError::from)?;
        self.inner.clear_lease(owner, server, &id, &attachment_id);
        Ok(())
    }

    pub async fn workbench_transfer(
        &self,
        source_owner: &str,
        target_owner: &str,
        server: &str,
        id: String,
        attachment_id: String,
    ) -> Result<WorkbenchAttached, ApiError> {
        let transition = self.inner.transition(server);
        let _guard = transition.lock().await;
        self.inner.reconcile(server).await?;
        let source = (source_owner.to_owned(), server.to_owned());
        let target = (target_owner.to_owned(), server.to_owned());
        let lease = {
            let leases = self.inner.leases.lock();
            if leases.0.contains_key(&target) {
                return Err(ApiError::InvalidArgument(
                    "Target window already owns a workbench on this remote".into(),
                ));
            }
            leases
                .0
                .get(&source)
                .filter(|lease| lease.id == id && lease.attachment_id == attachment_id)
                .cloned()
                .ok_or_else(|| not_controller(&id))?
        };
        let client = self.inner.client(server).await?;
        let attached = self
            .inner
            .recover_workbench(server, &client, id.clone(), attachment_id.clone())
            .await?
            .ok_or_else(|| not_controller(&id))?;
        if !matches!(&attached, WorkbenchAttached::Ready { controller_token, attachment_id: current, .. }
            if controller_token == &lease.controller_token && current == &attachment_id)
        {
            return Err(not_controller(&id));
        }
        self.inner.remote_runs.transfer_workbench(
            &self.inner.host,
            source_owner,
            target_owner,
            server,
            &id,
        )?;
        let mut leases = self.inner.leases.lock();
        leases.0.remove(&source);
        leases.0.insert(target, lease);
        Ok(attached)
    }

    /// Coordinator calls only after commit or rollback has reached its terminal
    /// owner, so observers cannot discard source state needed for rollback.
    pub fn notify_workbenches_changed(&self, server: &str) {
        if let Err(error) = (self.inner.events)(DesktopEvent::WorkbenchesChanged {
            server: server.to_owned(),
        }) {
            tracing::warn!(%server, %error, "local workbench transfer notification failed");
        }
    }

    pub async fn workbench_save(
        &self,
        server: &str,
        id: String,
        snapshot: String,
    ) -> Result<(), ApiError> {
        self.call_reply(server, Request::WorkbenchSave { id, snapshot })
            .await?
            .workbench_save()
            .map_err(ApiError::from)
    }
}

impl RouterInner {
    pub(super) fn transition(&self, server: &str) -> Arc<AsyncMutex<()>> {
        Arc::clone(
            self.transitions
                .lock()
                .entry(server.to_owned())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    /// Caller holds this server's transition gate. Failed observations never
    /// revoke local authority; only a successful list can prove it is gone.
    pub(super) async fn reconcile(&self, server: &str) -> Result<Vec<WorkbenchInfo>, ApiError> {
        let rows = self
            .call_reply(server, Request::WorkbenchList {})
            .await?
            .workbench_list()
            .map_err(ApiError::from)?;
        let mut leases = self.leases.lock();
        leases.0.retain(|(_, remote), lease| {
            remote != server || rows.iter().any(|row| row.id == lease.id && row.yours)
        });
        self.pending_stops.retain(&self.host, |stop| {
            stop.server != server
                || rows.iter().any(|row| {
                    row.id == stop.workbench
                        && ((!row.connected && !row.yours) || leases.current(stop))
                })
        });
        drop(leases);
        self.stop_events_if_idle(server);
        Ok(rows)
    }

    pub(super) async fn recover_workbench(
        &self,
        server: &str,
        client: &Arc<RemoteClient>,
        id: String,
        attachment_id: String,
    ) -> Result<Option<WorkbenchAttached>, ApiError> {
        match client
            .call(&Request::WorkbenchRecover { id, attachment_id })
            .await
        {
            Ok(reply) => reply.workbench_recover().map_err(ApiError::from),
            Err(error) => {
                if !matches!(&error, RemoteError::Wire(_)) {
                    // Failed recovery leaves admission unknown. Retiring this
                    // exact connection prevents untracked live control.
                    client.close();
                    self.evict(server, client).await;
                }
                Err(remote_error(server, error))
            }
        }
    }

    pub(super) fn clear_workbench(&self, server: &str, id: &str) {
        let mut leases = self.leases.lock();
        leases
            .0
            .retain(|(_, lease_server), lease| lease_server != server || lease.id != id);
        self.drop_unleased_stops(server, id, &leases);
        drop(leases);
        self.stop_events_if_idle(server);
    }

    pub(super) fn clear_lease(&self, owner: &str, server: &str, id: &str, attachment_id: &str) {
        let mut leases = self.leases.lock();
        let key = (owner.to_owned(), server.to_owned());
        if leases.owns(owner, server, id, attachment_id) {
            leases.0.remove(&key);
            self.drop_unleased_stops(server, id, &leases);
        }
        drop(leases);
        self.stop_events_if_idle(server);
    }

    pub(super) fn drop_unleased_stops(&self, server: &str, id: &str, leases: &Leases) {
        if leases.holder(server, id).is_some() {
            return;
        }
        // Runs remain listed in the workbench after this desktop releases its lease.
        self.pending_stops.retain(&self.host, |stop| {
            stop.server != server || stop.workbench != id
        });
    }

    pub(super) fn stop_events_if_idle(&self, server: &str) {
        if self.leases.lock().on_server(server) {
            return;
        }
        if let Some(slot) = self.remotes.lock().get(server).cloned() {
            if slot.claims.lock().is_empty() {
                slot.stop_events();
            }
        }
    }
}
