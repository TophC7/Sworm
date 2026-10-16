use super::{
    target::{remote_error, worktree_host_path},
    Target, WorkspaceRouter,
};
use crate::windows::WindowCoordinatorService;
use std::sync::Arc;
use sworm_core::errors::ApiError;
use sworm_protocol::{
    branch::BranchSummary,
    git::{GitWorktreeCheckout, GitWorktreeList, GitWorktreeOp, GitWorktreeResult},
    rpc::Request,
};

fn qualify_inventory(server: &str, inventory: &mut GitWorktreeList) {
    inventory.common_dir = Target::remote_uri(server, &inventory.common_dir);
    if let Some(path) = &mut inventory.main_path {
        *path = Target::remote_uri(server, path);
    }
    for worktree in &mut inventory.worktrees {
        worktree.path = Target::remote_uri(server, &worktree.path);
    }
}

fn inventory_roots(
    inventory: &GitWorktreeList,
    op: &GitWorktreeOp,
) -> Result<Vec<String>, ApiError> {
    let path = match op {
        GitWorktreeOp::Remove { worktree_path, .. } | GitWorktreeOp::Migrate { worktree_path } => {
            worktree_path
        }
        _ => return Ok(Vec::new()),
    };
    let member = inventory
        .worktrees
        .iter()
        .find(|row| &row.path == path)
        .ok_or_else(|| ApiError::InvalidArgument(format!("Not a registered worktree: {path}")))?;
    if member.is_main
        || member.bare
        || member.prunable.is_some()
        || (member.locked.is_some() && matches!(op, GitWorktreeOp::Remove { .. }))
    {
        return Err(ApiError::InvalidArgument(format!(
            "Cannot remove or migrate this worktree: {path}"
        )));
    }
    let mut roots = vec![member.path.clone()];
    if matches!(op, GitWorktreeOp::Migrate { .. }) {
        roots.push(inventory.main_path.clone().ok_or_else(|| {
            ApiError::InvalidArgument("This repository has no primary checkout".into())
        })?);
    }
    Ok(roots)
}

impl WorkspaceRouter {
    pub async fn git_worktree_list(&self, path: String) -> Result<GitWorktreeList, ApiError> {
        if let Target::Remote {
            server,
            path: native_path,
        } = Target::parse(&path)?
        {
            let mut inventory = self
                .call_reply(
                    server,
                    Request::GitWorktreeList {
                        path: native_path.to_owned(),
                    },
                )
                .await?
                .git_worktree_list()
                .map_err(ApiError::from)?;
            qualify_inventory(server, &mut inventory);
            return Ok(inventory);
        }
        self.local(move |host| host.git_worktree_list(&path)).await
    }

    pub async fn git_worktree_default_path(
        &self,
        path: String,
        checkout: GitWorktreeCheckout,
        root: Option<String>,
    ) -> Result<String, ApiError> {
        let root = root
            .as_deref()
            .map(|root| worktree_host_path(&path, root))
            .transpose()?;
        if let Target::Remote {
            server,
            path: native_path,
        } = Target::parse(&path)?
        {
            let destination = self
                .call_reply(
                    server,
                    Request::GitWorktreeDefaultPath {
                        path: native_path.to_owned(),
                        checkout,
                        root,
                    },
                )
                .await?
                .git_worktree_default_path()
                .map_err(ApiError::from)?;
            return Ok(Target::remote_uri(server, &destination));
        }
        self.local(move |host| host.git_worktree_default_path(&path, checkout, root))
            .await
    }

    pub async fn git_list_branches(&self, path: String) -> Result<Vec<BranchSummary>, ApiError> {
        if let Target::Remote {
            server,
            path: native_path,
        } = Target::parse(&path)?
        {
            let mut branches = self
                .call_reply(
                    server,
                    Request::GitListBranches {
                        path: native_path.to_owned(),
                    },
                )
                .await?
                .git_list_branches()
                .map_err(ApiError::from)?;
            for branch in &mut branches {
                if let Some(path) = &mut branch.worktree_path {
                    *path = Target::remote_uri(server, path);
                }
            }
            return Ok(branches);
        }
        self.local(move |host| host.git_list_branches(path)).await
    }

    /// This command owns its reservation and task through completion, even if
    /// its IPC waiter disappears. Destructive operations never reclaim `path`.
    pub async fn git_worktree(
        &self,
        owner: String,
        windows: Arc<WindowCoordinatorService>,
        path: String,
        op: GitWorktreeOp,
    ) -> Result<GitWorktreeResult, ApiError> {
        if windows.owner_closed(&owner) {
            return Err(ApiError::InvalidArgument(
                "The initiating window is closed".into(),
            ));
        }
        let destructive = matches!(
            &op,
            GitWorktreeOp::Remove { .. } | GitWorktreeOp::Migrate { .. }
        );
        let native_op = match &op {
            GitWorktreeOp::Add {
                destination,
                checkout,
            } => GitWorktreeOp::Add {
                destination: worktree_host_path(&path, destination)?,
                checkout: checkout.clone(),
            },
            GitWorktreeOp::Remove {
                worktree_path,
                force,
            } => GitWorktreeOp::Remove {
                worktree_path: worktree_host_path(&path, worktree_path)?,
                force: *force,
            },
            GitWorktreeOp::Migrate { worktree_path } => GitWorktreeOp::Migrate {
                worktree_path: worktree_host_path(&path, worktree_path)?,
            },
            GitWorktreeOp::Prune => GitWorktreeOp::Prune,
        };
        if let Target::Remote {
            server,
            path: native_path,
        } = Target::parse(&path)?
        {
            let server = server.to_owned();
            let native_path = native_path.to_owned();
            let reservation = if destructive {
                let inventory = self.git_worktree_list(path.clone()).await?;
                Some(
                    windows
                        .reserve_worktree_roots(&inventory_roots(&inventory, &op)?)
                        .map_err(ApiError::InvalidArgument)?,
                )
            } else {
                None
            };
            let router = self.clone();
            return tokio::spawn(async move {
                let _reservation = reservation;
                let workbench = router
                    .inner
                    .leases
                    .lock()
                    .0
                    .get(&(owner, server.clone()))
                    .map(|lease| lease.id.clone());
                // Never replay a mutation after a transport failure: Git may
                // already have completed it without delivering its response.
                let client = router.inner.client(&server).await?;
                let request = Request::GitWorktree {
                    path: native_path,
                    op: native_op,
                };
                let reply = match workbench {
                    Some(id) => client.call_workbench(&id, &request).await,
                    None => client.call(&request).await,
                }
                .map_err(|error| remote_error(&server, error))?;
                let mut result = reply.git_worktree().map_err(ApiError::from)?;
                match &mut result {
                    GitWorktreeResult::Added { path, .. } => {
                        *path = Target::remote_uri(&server, path)
                    }
                    GitWorktreeResult::Migrated { main_path, .. } => {
                        *main_path = Target::remote_uri(&server, main_path)
                    }
                    _ => {}
                }
                Ok(result)
            })
            .await?;
        }
        if !destructive {
            return self
                .local(move |host| host.git_worktree(&path, native_op))
                .await;
        }
        let preflight_path = path.clone();
        let preflight_op = native_op.clone();
        let roots = self
            .local(move |host| host.git_worktree_affected_roots(&preflight_path, &preflight_op))
            .await?;
        let reservation = windows
            .reserve_worktree_roots(&roots)
            .map_err(ApiError::InvalidArgument)?;
        let host = Arc::clone(&self.inner.host);
        tokio::spawn(async move {
            let admission = Arc::clone(&host.admission).write_owned().await;
            tokio::task::spawn_blocking(move || {
                let _reservation = reservation;
                let _admission = admission;
                let current = host.git_worktree_affected_roots(&path, &native_op)?;
                if current != roots {
                    return Err(ApiError::InvalidArgument(
                        "Worktree ownership roots changed; refresh and try again".into(),
                    ));
                }
                host.drain_worktree_roots(&current)?;
                host.git_worktree(&path, native_op)
            })
            .await?
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sworm_protocol::rpc::WireError;
    use sworm_remote::RemoteError;

    #[test]
    fn nested_worktree_paths_never_cross_hosts() {
        assert_eq!(
            worktree_host_path("sworm://one/repo", "sworm://one/new").unwrap(),
            "/new"
        );
        for path in ["/new", "sworm://two/new"] {
            assert!(worktree_host_path("sworm://one/repo", path).is_err());
        }
        assert!(worktree_host_path("/repo", "sworm://one/new").is_err());
    }

    #[test]
    fn destructive_git_errors_keep_remote_host_identity() {
        let error = remote_error(
            "one",
            RemoteError::Wire(WireError::BranchInWorktree {
                branch: "topic".into(),
                path: "/worktrees/topic".into(),
            }),
        );
        assert!(
            matches!(error, ApiError::BranchInWorktree { path, .. } if path == "sworm://one/worktrees/topic")
        );
        let error = remote_error(
            "one",
            RemoteError::Wire(WireError::WorktreeHasChanges {
                path: "/worktrees/topic".into(),
                message: "dirty".into(),
            }),
        );
        assert!(
            matches!(error, ApiError::WorktreeHasChanges { path, .. } if path == "sworm://one/worktrees/topic")
        );
    }

    #[tokio::test]
    async fn cancelled_local_waiter_does_not_unfence_blocking_host_work() {
        let folder = tempfile::tempdir().unwrap();
        let host = Arc::new(
            sworm_core::Host::new(folder.path().join("state.db"), Arc::new(|_| Ok(()))).unwrap(),
        );
        let router = WorkspaceRouter::new(Arc::clone(&host));
        let (entered, started) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let operation = tokio::spawn(async move {
            router
                .local(move |host| {
                    entered.send(()).unwrap();
                    blocked.recv().unwrap();
                    host.app_state_put("admission-test".into(), "\"completed\"".into())
                })
                .await
        });
        started.await.unwrap();
        operation.abort();
        assert!(operation.await.unwrap_err().is_cancelled());
        assert!(host.admission.try_write().is_err());
        release.send(()).unwrap();
        let writer =
            tokio::time::timeout(std::time::Duration::from_secs(2), host.admission.write())
                .await
                .unwrap();
        assert_eq!(
            host.app_state_get("admission-test".into()).unwrap(),
            Some("\"completed\"".into())
        );
        drop(writer);
    }
}
