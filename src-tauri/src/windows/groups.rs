use super::*;
use sworm_protocol::rpc::WorkbenchAttached;
use tokio::sync::{oneshot, watch};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupHandoff {
    pub transfer_id: String,
    pub source_window: String,
    pub target_window: String,
    pub server: String,
    pub workbench_id: String,
    pub index: usize,
}

#[derive(Debug)]
enum Phase {
    WaitingReady,
    Requested,
    Exported {
        attachment_id: String,
        tabs: Vec<serde_json::Value>,
    },
    Finished,
}

pub(super) struct GroupTransfer {
    handoff: GroupHandoff,
    deadline: tokio::time::Instant,
    phase: tokio::sync::Mutex<Phase>,
    cancel: watch::Sender<Option<String>>,
    completion: Mutex<Option<oneshot::Sender<Result<(), String>>>>,
}

impl GroupTransfer {
    fn new(handoff: GroupHandoff) -> (Arc<Self>, oneshot::Receiver<Result<(), String>>) {
        let (completion, result) = oneshot::channel();
        let (cancel, _) = watch::channel(None);
        (
            Arc::new(Self {
                handoff,
                deadline: tokio::time::Instant::now() + TRANSFER_TIMEOUT,
                phase: tokio::sync::Mutex::new(Phase::WaitingReady),
                cancel,
                completion: Mutex::new(Some(completion)),
            }),
            result,
        )
    }

    fn authorize(&self, caller: &str) -> Result<(), String> {
        if caller == self.handoff.source_window || caller == self.handoff.target_window {
            Ok(())
        } else {
            Err("window is not a group transfer participant".into())
        }
    }

    fn finish(&self, result: Result<(), String>) {
        if let Some(completion) = self.completion.lock().take() {
            let _ = completion.send(result);
        }
    }
}

impl WindowCoordinatorService {
    pub async fn group_handoff(
        &self,
        app: &tauri::AppHandle,
        caller: &str,
        source_window: String,
        server: String,
        workbench_id: String,
        index: usize,
        target_window: Option<String>,
    ) -> Result<(), String> {
        authorize_initiation(caller, &source_window, target_window.as_deref())?;
        if server.is_empty() || workbench_id.is_empty() {
            return Err("group identity must not be empty".into());
        }
        if !self.has_window(&source_window) {
            return Err("source window no longer exists".into());
        }
        let target_window = match target_window {
            Some(target) => target,
            None => self.create_workbench_window(app, None)?.label().to_owned(),
        };
        let handoff = GroupHandoff {
            transfer_id: format!("group-transfer-{}", uuid::Uuid::new_v4()),
            source_window,
            target_window,
            server,
            workbench_id,
            index,
        };
        let (transfer, result) = self.insert_group(handoff)?;
        self.request_ready_groups(app, &transfer.handoff.target_window);
        let timeout_app = app.clone();
        let pending = Arc::downgrade(&transfer);
        let deadline = transfer.deadline;
        // Timeout outlives its invoke future; closing the requester cannot leave
        // a transaction available for a late stage reply.
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let (Some(transfer), Some(state)) =
                (pending.upgrade(), timeout_app.try_state::<AppState>())
            {
                state
                    .windows
                    .cancel_group(&timeout_app, &transfer, "group transfer timed out")
                    .await;
            }
        });
        result
            .await
            .map_err(|_| "group transfer completion lost".to_string())?
    }

    fn insert_group(
        &self,
        handoff: GroupHandoff,
    ) -> Result<(Arc<GroupTransfer>, oneshot::Receiver<Result<(), String>>), String> {
        let mut transfers = self.group_transfers.lock();
        let records = self.records.lock();
        if handoff.source_window == handoff.target_window {
            return Err("group transfer requires different windows".into());
        }
        if !records.contains_key(&handoff.source_window)
            || !records.contains_key(&handoff.target_window)
        {
            return Err("group transfer participant no longer exists".into());
        }
        if transfers.values().any(|transfer| {
            let active = &transfer.handoff;
            active.server == handoff.server
                && (active.workbench_id == handoff.workbench_id
                    || active.target_window == handoff.target_window
                    || active.source_window == handoff.target_window
                    || active.target_window == handoff.source_window)
        }) {
            return Err("group transfer already in progress".into());
        }
        let (transfer, result) = GroupTransfer::new(handoff);
        transfers.insert(transfer.handoff.transfer_id.clone(), transfer.clone());
        Ok((transfer, result))
    }

    pub(super) fn request_ready_groups<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        label: &str,
    ) {
        let transfers: Vec<_> = self
            .group_transfers
            .lock()
            .values()
            .filter(|transfer| transfer.handoff.target_window == label)
            .cloned()
            .collect();
        if !self
            .records
            .lock()
            .get(label)
            .is_some_and(|record| record.ready)
        {
            return;
        }
        for transfer in transfers {
            let Ok(mut phase) = transfer.phase.try_lock() else {
                continue;
            };
            if !matches!(*phase, Phase::WaitingReady) {
                continue;
            }
            if transfer.cancel.borrow().is_some()
                || tokio::time::Instant::now() >= transfer.deadline
            {
                continue;
            }
            *phase = Phase::Requested;
            if let Err(error) = emit_group_event(
                app,
                &transfer.handoff.source_window,
                "group-transfer-request",
                &transfer.handoff,
            ) {
                self.abort_group_locked(
                    app,
                    &transfer,
                    &mut phase,
                    &format!("failed to request group export: {error}"),
                );
            }
        }
    }

    fn group(&self, id: &str) -> Result<Arc<GroupTransfer>, String> {
        self.group_transfers
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| "unknown or expired group transfer".into())
    }

    pub async fn group_exported(
        &self,
        app: &tauri::AppHandle,
        source: &str,
        transfer_id: &str,
        attachment_id: String,
        tabs: Vec<serde_json::Value>,
        active_tab_id: Option<String>,
        model_states: Vec<serde_json::Value>,
    ) -> Result<(), String> {
        let transfer = self.group(transfer_id)?;
        if transfer.handoff.source_window != source {
            return Err("group export belongs to a different window".into());
        }
        let mut phase = transfer.phase.lock().await;
        let result = (|| {
            ensure_pending(&transfer, &phase)?;
            if !matches!(*phase, Phase::Requested) {
                return Err("group source export already received or target not ready".into());
            }
            let state = app.state::<AppState>();
            let handoff = &transfer.handoff;
            if !state.router.workbench_owned(
                source,
                &handoff.server,
                &handoff.workbench_id,
                &attachment_id,
            ) {
                return Err("group export attachment identity is stale".into());
            }
            validate_metadata(
                &handoff.server,
                &tabs,
                active_tab_id.as_deref(),
                &model_states,
            )?;
            let records = self.records.lock();
            let source = records.get(source).ok_or("source window closed")?;
            for tab in &tabs {
                let folder = tab["folderPath"].as_str().expect("validated folder");
                if !source.folder_claims.contains(Path::new(folder)) {
                    return Err("exported tab folder is not owned by source window".into());
                }
            }
            drop(records);
            let mut payload = serde_json::to_value(handoff).map_err(|error| error.to_string())?;
            payload["tabs"] = serde_json::to_value(&tabs).map_err(|error| error.to_string())?;
            payload["activeTabId"] = serde_json::json!(active_tab_id);
            payload["modelStates"] = serde_json::json!(model_states);
            *phase = Phase::Exported {
                attachment_id,
                tabs,
            };
            emit_group_event(
                app,
                &handoff.target_window,
                "group-transfer-import",
                &payload,
            )
            .map_err(|error| format!("failed to stage group: {error}"))
        })();
        if let Err(reason) = &result {
            self.abort_group_locked(app, &transfer, &mut phase, reason);
        }
        result
    }

    pub async fn group_staged(
        &self,
        app: &tauri::AppHandle,
        target: &str,
        id: &str,
    ) -> Result<(), String> {
        let transfer = self.group(id)?;
        if transfer.handoff.target_window != target {
            return Err("group stage belongs to a different window".into());
        }
        let mut phase = transfer.phase.lock().await;
        let result = self.commit_group(app, &transfer, &mut phase).await;
        if let Err(reason) = &result {
            if !matches!(*phase, Phase::Finished) {
                self.abort_group_locked(app, &transfer, &mut phase, reason);
            }
        }
        result
    }

    async fn commit_group(
        &self,
        app: &tauri::AppHandle,
        transfer: &GroupTransfer,
        phase: &mut Phase,
    ) -> Result<(), String> {
        ensure_pending(transfer, phase)?;
        let Phase::Exported {
            attachment_id,
            tabs,
        } = phase
        else {
            return Err("group has not been exported".into());
        };
        let handoff = &transfer.handoff;
        let state = app.state::<AppState>();
        if !self.has_window(&handoff.source_window)
            || !self.has_window(&handoff.target_window)
            || app.get_webview_window(&handoff.source_window).is_none()
            || app.get_webview_window(&handoff.target_window).is_none()
        {
            return Err("group transfer participant closed".into());
        }
        let mut cancelled = transfer.cancel.subscribe();
        if let Some(reason) = cancelled.borrow().clone() {
            return Err(reason);
        }
        let attached = tokio::select! {
            biased;
            _ = cancelled.changed() => return Err(cancelled.borrow().clone().unwrap_or_else(|| "group transfer cancelled".into())),
            _ = tokio::time::sleep_until(transfer.deadline) => return Err("group transfer timed out".into()),
            attached = state.router.workbench_transfer(&handoff.source_window, &handoff.target_window,
                &handoff.server, handoff.workbench_id.clone(), attachment_id.clone()) => attached.map_err(|error| error.to_string())?,
        };
        if !matches!(attached, WorkbenchAttached::Ready { .. }) {
            return Err("group transfer did not return an attached workbench".into());
        }
        let mut finalized = serde_json::to_value(handoff).map_err(|error| error.to_string())?;
        finalized["attached"] =
            serde_json::to_value(&attached).map_err(|error| error.to_string())?;
        // Until target receives Ready, source retains its models. Rollback uses the
        // exact attachment identity; never detach or take over a newer controller.
        if let Err(error) = emit_group_event(
            app,
            &handoff.target_window,
            "group-transfer-finalized",
            &finalized,
        ) {
            if state.router.workbench_owned(
                &handoff.target_window,
                &handoff.server,
                &handoff.workbench_id,
                attachment_id,
            ) && self.has_window(&handoff.source_window)
                && app.get_webview_window(&handoff.source_window).is_some()
                && matches!(
                    tokio::time::timeout_at(
                        transfer.deadline,
                        state.router.workbench_transfer(
                            &handoff.target_window,
                            &handoff.source_window,
                            &handoff.server,
                            handoff.workbench_id.clone(),
                            attachment_id.clone()
                        )
                    )
                    .await,
                    Ok(Ok(_))
                )
            {
                return Err(format!(
                    "group finalization failed; source retained control: {error}"
                ));
            }
            // Ownership cannot safely be restored. Tell source commit won so it
            // cannot resurrect stale control; requester still gets delivery failure.
            self.commit_group_claims(handoff, tabs);
            let _ = emit_group_event(
                app,
                &handoff.source_window,
                "group-transfer-committed",
                handoff,
            );
            let result = Err(format!(
                "group committed but target notification failed: {error}"
            ));
            return self.finish_group(app, transfer, phase, Some(&attached), result);
        }
        self.commit_group_claims(handoff, tabs);
        let result = emit_group_event(
            app,
            &handoff.source_window,
            "group-transfer-committed",
            handoff,
        )
        .map_err(|error| format!("group committed but source notification failed: {error}"));
        self.finish_group(app, transfer, phase, Some(&attached), result)
    }

    fn commit_group_claims(&self, handoff: &GroupHandoff, tabs: &[serde_json::Value]) {
        let mut records = self.records.lock();
        let ids: HashSet<_> = tabs.iter().filter_map(|tab| tab["id"].as_str()).collect();
        let folders: HashSet<_> = tabs
            .iter()
            .filter_map(|tab| tab["folderPath"].as_str())
            .map(PathBuf::from)
            .collect();
        let files = if let Some(source) = records.get_mut(&handoff.source_window) {
            source
                .folder_claims
                .retain(|folder| !folders.contains(folder));
            let files: Vec<_> = source
                .file_claims
                .iter()
                .filter(|(_, id)| ids.contains(id.as_str()))
                .map(|(path, id)| (path.clone(), id.clone()))
                .collect();
            for (path, _) in &files {
                source.file_claims.remove(path);
            }
            files
        } else {
            Vec::new()
        };
        if let Some(target) = records.get_mut(&handoff.target_window) {
            target.folder_claims.extend(folders);
            target.file_claims.extend(files);
        }
    }

    fn finish_group<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        transfer: &GroupTransfer,
        phase: &mut Phase,
        attached: Option<&WorkbenchAttached>,
        mut result: Result<(), String>,
    ) -> Result<(), String> {
        *phase = Phase::Finished;
        self.group_transfers
            .lock()
            .remove(&transfer.handoff.transfer_id);
        let mut payload = serde_json::to_value(&transfer.handoff).expect("handoff serialization");
        payload["committed"] = serde_json::json!(attached.is_some());
        if let Some(attached) = attached {
            payload["attached"] = serde_json::to_value(attached).expect("attached serialization");
        }
        if let Err(reason) = &result {
            payload["reason"] = serde_json::json!(reason);
        }
        // Global delivery is the terminal fallback for a missed window event.
        // Publish list reconciliation only after rollback/commit is settled.
        if let Err(error) = app.emit("group-transfer-settled", payload) {
            let failure = format!("group terminal settlement notification failed: {error}");
            result = Err(match result {
                Ok(()) => failure,
                Err(reason) => format!("{reason}; {failure}"),
            });
        }
        if let Some(state) = app.try_state::<AppState>() {
            state
                .router
                .notify_workbenches_changed(&transfer.handoff.server);
        }
        transfer.finish(result.clone());
        result
    }

    fn abort_group_locked<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        transfer: &GroupTransfer,
        phase: &mut Phase,
        reason: &str,
    ) {
        if matches!(*phase, Phase::Finished) {
            return;
        }
        let mut payload = serde_json::to_value(&transfer.handoff).expect("handoff serialization");
        payload["reason"] = serde_json::json!(reason);
        for recipient in [
            &transfer.handoff.source_window,
            &transfer.handoff.target_window,
        ] {
            if let Err(error) = app.emit_to(recipient, "group-transfer-aborted", &payload) {
                tracing::warn!(%error, %recipient, "group abort notification failed");
            }
        }
        let _ = self.finish_group(app, transfer, phase, None, Err(reason.to_owned()));
    }

    async fn cancel_group<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        transfer: &GroupTransfer,
        reason: &str,
    ) {
        transfer.cancel.send_replace(Some(reason.to_owned()));
        let mut phase = transfer.phase.lock().await;
        self.abort_group_locked(app, transfer, &mut phase, reason);
    }

    pub async fn group_abort(
        &self,
        app: &tauri::AppHandle,
        caller: &str,
        id: &str,
        reason: &str,
    ) -> Result<(), String> {
        let transfer = self.group(id)?;
        transfer.authorize(caller)?;
        self.cancel_group(app, &transfer, reason).await;
        Ok(())
    }

    pub(super) fn cancel_groups_for_window(&self, label: &str) {
        for transfer in self
            .group_transfers
            .lock()
            .values()
            .filter(|transfer| transfer.authorize(label).is_ok())
        {
            transfer
                .cancel
                .send_replace(Some("group transfer participant closed".into()));
        }
    }

    pub(super) async fn abort_groups_for_window<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        label: &str,
    ) {
        let transfers: Vec<_> = self
            .group_transfers
            .lock()
            .values()
            .filter(|transfer| transfer.authorize(label).is_ok())
            .cloned()
            .collect();
        for transfer in transfers {
            self.cancel_group(app, &transfer, "group transfer participant closed")
                .await;
        }
    }
}

fn emit_group_event<R: tauri::Runtime, P: Serialize>(
    app: &tauri::AppHandle<R>,
    label: &str,
    event: &str,
    payload: &P,
) -> Result<(), String> {
    if app.get_webview_window(label).is_none() {
        return Err(format!("window closed: {label}"));
    }
    app.emit_to(label, event, payload)
        .map_err(|error| error.to_string())
}

fn authorize_initiation(caller: &str, source: &str, target: Option<&str>) -> Result<(), String> {
    if target.map_or(caller == source, |target| caller == target) {
        Ok(())
    } else {
        Err("group handoff caller is not its requesting participant".into())
    }
}

fn ensure_pending(transfer: &GroupTransfer, phase: &Phase) -> Result<(), String> {
    if matches!(phase, Phase::Finished) || transfer.cancel.borrow().is_some() {
        return Err("group transfer was cancelled".into());
    }
    if tokio::time::Instant::now() >= transfer.deadline {
        return Err("group transfer timed out".into());
    }
    Ok(())
}

fn validate_metadata(
    server: &str,
    tabs: &[serde_json::Value],
    active: Option<&str>,
    models: &[serde_json::Value],
) -> Result<(), String> {
    let mut ids = HashMap::new();
    for tab in tabs {
        let id = tab
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or("invalid exported tab id")?;
        let folder = tab
            .get("folderPath")
            .and_then(serde_json::Value::as_str)
            .ok_or("invalid exported folder")?;
        let crate::router::Target::Remote {
            server: tab_server, ..
        } = crate::router::Target::parse(folder).map_err(|error| error.to_string())?
        else {
            return Err("exported tab is not in a server workbench".into());
        };
        if tab_server != server {
            return Err("exported tab belongs to a different server".into());
        }
        if !matches!(
            tab.get("kind").and_then(serde_json::Value::as_str),
            Some("session" | "task" | "text" | "diff" | "tool" | "launcher" | "issue" | "epic")
        ) || !tab.get("locked").is_some_and(serde_json::Value::is_boolean)
        {
            return Err("invalid exported tab schema".into());
        }
        if tab["kind"] == "text"
            && (!tab
                .get("filePath")
                .is_some_and(|path| path.is_null() || path.is_string())
                || !tab
                    .get("fileName")
                    .is_some_and(serde_json::Value::is_string))
        {
            return Err("invalid exported text tab schema".into());
        }
        if matches!(tab["kind"].as_str(), Some("session" | "task"))
            && !tab
                .get("runId")
                .is_some_and(|id| id.is_string() || (tab["kind"] == "session" && id.is_null()))
        {
            return Err("invalid exported terminal tab schema".into());
        }
        if ids.insert(id, tab).is_some() {
            return Err("duplicate exported tab identity".into());
        }
    }
    if active.is_some_and(|id| !ids.contains_key(id)) {
        return Err("active tab is not in exported group".into());
    }
    let mut model_ids = HashSet::new();
    for model in models {
        let id = model
            .get("tabId")
            .and_then(serde_json::Value::as_str)
            .ok_or("invalid model tab identity")?;
        let tab = ids
            .get(id)
            .ok_or("model does not belong to exported group")?;
        if tab["kind"] != "text"
            || !model_ids.insert(id)
            || model.get("folderPath") != tab.get("folderPath")
            || model.get("filePath") != tab.get("filePath")
            || !model.get("value").is_some_and(serde_json::Value::is_string)
            || !model
                .get("savedValue")
                .is_some_and(serde_json::Value::is_string)
            || !model
                .get("language")
                .is_some_and(serde_json::Value::is_string)
        {
            return Err("model metadata does not match exported text tab".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Listener;

    fn handoff(id: &str) -> GroupHandoff {
        GroupHandoff {
            transfer_id: id.into(),
            source_window: "source".into(),
            target_window: "target".into(),
            server: "dev".into(),
            workbench_id: "wb".into(),
            index: 2,
        }
    }

    fn coordinator() -> WindowCoordinatorService {
        let service = WindowCoordinatorService::new();
        for label in ["source", "target"] {
            service.records.lock().insert(
                label.into(),
                LiveWindowRecord {
                    label: label.into(),
                    bounds: None,
                    maximized: false,
                    focus_order: 0,
                    ready: false,
                    pending_open_targets: Vec::new(),
                    folder_claims: HashSet::from([PathBuf::from("sworm://dev/repo")]),
                    file_claims: HashMap::new(),
                },
            );
        }
        service
    }

    #[test]
    fn drag_and_new_window_require_the_requesting_participant() {
        assert!(authorize_initiation("target", "source", Some("target")).is_ok());
        assert!(authorize_initiation("source", "source", None).is_ok());
        assert!(authorize_initiation("source", "source", Some("target")).is_err());
        assert!(authorize_initiation("target", "source", None).is_err());
        assert!(authorize_initiation("stranger", "source", Some("target")).is_err());
    }

    #[tokio::test]
    async fn destination_startup_and_abort_leave_source_metadata_intact() {
        let service = coordinator();
        let app = tauri::test::mock_app();
        let source = WebviewWindowBuilder::new(&app, "source", Default::default())
            .build()
            .unwrap();
        let target = WebviewWindowBuilder::new(&app, "target", Default::default())
            .build()
            .unwrap();
        let (send, events) = std::sync::mpsc::channel();
        for (window, event) in [
            (&source, "group-transfer-request"),
            (&source, "group-transfer-aborted"),
            (&target, "group-transfer-aborted"),
        ] {
            let send = send.clone();
            let label = window.label().to_owned();
            window.listen(event, move |_| {
                send.send((label.clone(), event)).unwrap();
            });
        }
        let (transfer, mut completion) = service.insert_group(handoff("startup")).unwrap();
        service.request_ready_groups(app.handle(), "target");
        assert!(
            events.try_recv().is_err(),
            "source cannot export before destination readiness"
        );
        assert!(matches!(
            completion.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        service.records.lock().get_mut("target").unwrap().ready = true;
        service.request_ready_groups(app.handle(), "target");
        assert_eq!(
            events.recv_timeout(Duration::from_secs(1)).unwrap(),
            ("source".into(), "group-transfer-request")
        );
        service
            .cancel_group(app.handle(), &transfer, "destination startup failed")
            .await;
        assert_eq!(
            completion.await.unwrap(),
            Err("destination startup failed".into())
        );
        let mut aborted = HashSet::new();
        for _ in 0..2 {
            let (label, event) = events.recv_timeout(Duration::from_secs(1)).unwrap();
            assert_eq!(event, "group-transfer-aborted");
            aborted.insert(label);
        }
        assert_eq!(aborted, HashSet::from(["source".into(), "target".into()]));
        assert!(service.records.lock()["source"]
            .folder_claims
            .contains(Path::new("sworm://dev/repo")));
        assert!(
            service.group("startup").is_err(),
            "late stage/export cannot find aborted transfer"
        );
        assert!(matches!(*transfer.phase.lock().await, Phase::Finished));
    }

    #[tokio::test]
    async fn timeout_invalidates_export_and_completion_without_target_readiness() {
        let service = coordinator();
        let app = tauri::test::mock_app();
        let (mut transfer, completion) = GroupTransfer::new(handoff("expired"));
        Arc::get_mut(&mut transfer).unwrap().deadline =
            tokio::time::Instant::now() - Duration::from_secs(1);
        service
            .group_transfers
            .lock()
            .insert("expired".into(), transfer.clone());
        assert_eq!(
            ensure_pending(&transfer, &Phase::Requested).unwrap_err(),
            "group transfer timed out"
        );
        service
            .cancel_group(app.handle(), &transfer, "group transfer timed out")
            .await;
        assert_eq!(
            completion.await.unwrap(),
            Err("group transfer timed out".into())
        );
        assert!(service.group("expired").is_err());
    }

    #[tokio::test]
    async fn destroying_either_participant_aborts_exported_group() {
        for label in ["source", "target"] {
            let service = coordinator();
            let app = tauri::test::mock_app();
            let (transfer, completion) = service.insert_group(handoff(label)).unwrap();
            assert!(transfer.authorize("stranger").is_err());
            *transfer.phase.lock().await = Phase::Exported {
                attachment_id: "attachment".into(),
                tabs: Vec::new(),
            };
            service.abort_groups_for_window(app.handle(), label).await;
            assert_eq!(
                completion.await.unwrap(),
                Err("group transfer participant closed".into())
            );
            assert!(service.group(label).is_err());
        }
    }

    #[tokio::test]
    async fn terminal_delivery_failure_does_not_misreport_commit_as_abort() {
        let service = coordinator();
        let app = tauri::test::mock_app();
        let (send, settled) = std::sync::mpsc::channel();
        app.listen("group-transfer-settled", move |event| {
            send.send(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap())
                .unwrap();
        });
        let (transfer, completion) = service.insert_group(handoff("committed")).unwrap();
        let attached = WorkbenchAttached::Ready {
            attachment_id: "identity".into(),
            controller_token: "controller".into(),
            snapshot: "{\"fresh\":true}".into(),
        };
        let failure = Err("group committed but source notification failed".into());
        let mut phase = transfer.phase.lock().await;
        assert_eq!(
            service.finish_group(
                app.handle(),
                &transfer,
                &mut phase,
                Some(&attached),
                failure.clone()
            ),
            failure
        );
        drop(phase);
        assert_eq!(completion.await.unwrap(), failure);
        let terminal = settled.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(terminal["committed"], true);
        assert_eq!(terminal["attached"]["attachment_id"], "identity");
        assert_eq!(terminal["attached"]["snapshot"], "{\"fresh\":true}");
        assert!(service.group("committed").is_err());
        assert!(ensure_pending(&transfer, &*transfer.phase.lock().await).is_err());
    }

    #[test]
    fn raw_group_metadata_preserves_untitled_text_and_rejects_cross_group_identity() {
        let tab = serde_json::json!({
            "id": "text", "kind": "text", "folderPath": "sworm://dev/repo",
            "filePath": null, "fileName": "Untitled", "locked": false,
        });
        let model = serde_json::json!({
            "tabId": "text", "folderPath": "sworm://dev/repo", "filePath": null,
            "value": "unsaved", "savedValue": "", "language": "plaintext", "viewState": null,
        });
        assert!(validate_metadata(
            "dev",
            std::slice::from_ref(&tab),
            Some("text"),
            std::slice::from_ref(&model)
        )
        .is_ok());
        assert!(validate_metadata(
            "other",
            std::slice::from_ref(&tab),
            Some("text"),
            std::slice::from_ref(&model)
        )
        .is_err());
        assert!(validate_metadata("dev", &[tab.clone(), tab.clone()], None, &[]).is_err());
        assert!(validate_metadata("dev", std::slice::from_ref(&tab), Some("other"), &[]).is_err());
        let mut foreign = model.clone();
        foreign["folderPath"] = serde_json::json!("sworm://dev/other");
        assert!(validate_metadata("dev", std::slice::from_ref(&tab), None, &[foreign]).is_err());
        foreign = model.clone();
        foreign["tabId"] = serde_json::json!("other");
        assert!(validate_metadata("dev", std::slice::from_ref(&tab), None, &[foreign]).is_err());
        assert!(validate_metadata(
            "dev",
            std::slice::from_ref(&tab),
            None,
            &[model.clone(), model]
        )
        .is_err());
        let mut malformed = tab;
        malformed["filePath"] = serde_json::json!(42);
        assert!(validate_metadata("dev", &[malformed], None, &[]).is_err());
    }
}
