//! Per-project issue store backed by `.sworm/issues.db` (SQLite, WAL).
//!
//! Owns the schema, an LRU of per-project [`IssueProjectDb`] handles, and
//! the full read/write API consumed by Tauri commands and the bridge.
//! Each project DB carries one writer connection and a small reader pool
//! mirroring [`crate::services::db::DatabaseService`], so list/search/get
//! never serialize behind a long write.
//!
//! The implementation is split across submodules so each domain — issues,
//! epics, comments, dependencies, config — owns its own file. They all
//! extend the same [`IssueService`] struct via separate `impl` blocks.

mod comment_ops;
mod config_ops;
mod db;
mod dependency_ops;
mod epic_ops;
mod issue_ops;
mod queries;
mod rows;
mod validators;

use db::{open_issue_connection, IssueProjectDb, ISSUE_DB_REL, READ_POOL_SIZE, SCHEMA_SQL};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

pub(super) const DEFAULT_LIMIT: i64 = 100;

/// Project-aware facade over [`IssueProjectDb`] handles. All public
/// methods are sync; commands that need to stay off the Tauri runtime
/// thread should call them inside `tokio::task::spawn_blocking`.
pub struct IssueService {
    dbs: Mutex<HashMap<PathBuf, Arc<IssueProjectDb>>>,
}

impl IssueService {
    pub fn new() -> Self {
        Self {
            dbs: Mutex::new(HashMap::new()),
        }
    }

    /// Path to the issue DB inside `project_path`. Public so tests and
    /// startup code can verify the file location without opening the DB.
    pub fn issue_db_path(project_path: &Path) -> PathBuf {
        project_path.join(ISSUE_DB_REL)
    }

    /// Resolve (and lazily create) the [`IssueProjectDb`] for a
    /// project. Called by every op submodule.
    pub(in crate::services::issues) fn db(
        &self,
        project_path: &Path,
    ) -> Result<Arc<IssueProjectDb>, String> {
        let db_path = Self::issue_db_path(project_path);
        if let Some(existing) = self.dbs.lock().get(&db_path).cloned() {
            return Ok(existing);
        }

        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to create issue db directory {}: {}",
                    parent.display(),
                    e
                )
            })?;
        }

        let writer = open_issue_connection(&db_path, false)?;
        writer
            .execute_batch(SCHEMA_SQL)
            .map_err(|e| format!("Failed to initialize issue db {}: {}", db_path.display(), e))?;

        let mut readers = Vec::with_capacity(READ_POOL_SIZE);
        for _ in 0..READ_POOL_SIZE {
            readers.push(Mutex::new(open_issue_connection(&db_path, true)?));
        }

        let db = Arc::new(IssueProjectDb {
            writer: Mutex::new(writer),
            readers,
            read_cursor: AtomicUsize::new(0),
        });
        self.dbs.lock().insert(db_path, db.clone());
        Ok(db)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::issues::*;
    use uuid::Uuid;

    fn temp_project(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("sworm-issues-test-{}-{}", name, Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn create_test_epic(service: &IssueService, project: &Path) -> IssueEpic {
        service
            .create_epic(
                project,
                IssueEpicCreateInput {
                    title: "Epic".into(),
                    description: None,
                    status: None,
                    priority: None,
                    actor: None,
                },
            )
            .unwrap()
    }

    #[test]
    fn creates_project_local_db_and_transactional_ids() {
        let service = IssueService::new();
        let project = temp_project("ids");
        let epic = create_test_epic(&service, &project);
        let first = service
            .create(
                &project,
                IssueCreateInput {
                    title: "First".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        let second = service
            .create(
                &project,
                IssueCreateInput {
                    title: "Second".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        assert!(IssueService::issue_db_path(&project).exists());
        assert_eq!(first.id, "ISSUE-1");
        assert_eq!(second.id, "ISSUE-2");
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn requires_epic_for_root_issue_and_inherits_parent_epic_for_subissue() {
        let service = IssueService::new();
        let project = temp_project("hierarchy");
        let epic = create_test_epic(&service, &project);

        let rootless = service.create(
            &project,
            IssueCreateInput {
                title: "Rootless".into(),
                description: None,
                status: None,
                priority: None,
                epic_id: None,
                parent_issue_id: None,
                assignee_kind: None,
                assignee_id: None,
                tags: vec![],
                context_json: None,
                actor: None,
            },
        );
        assert!(rootless.is_err());

        let parent = service
            .create(
                &project,
                IssueCreateInput {
                    title: "Parent".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        let subissue = service
            .create(
                &project,
                IssueCreateInput {
                    title: "Sub".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: None,
                    parent_issue_id: Some(parent.id.clone()),
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        assert_eq!(subissue.epic_id.as_deref(), Some(epic.id.as_str()));
        assert_eq!(
            subissue.parent_issue_id.as_deref(),
            Some(parent.id.as_str())
        );
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn dependency_cycle_and_duplicates_rejected() {
        let service = IssueService::new();
        let project = temp_project("deps");
        let epic = create_test_epic(&service, &project);
        let a = service
            .create(
                &project,
                IssueCreateInput {
                    title: "A".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        let b = service
            .create(
                &project,
                IssueCreateInput {
                    title: "B".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        service
            .add_dependency(
                &project,
                IssueDependencyInput {
                    issue_id: b.id.clone(),
                    depends_on_issue_id: a.id.clone(),
                    actor: None,
                },
            )
            .unwrap();
        assert!(service
            .add_dependency(
                &project,
                IssueDependencyInput {
                    issue_id: b.id.clone(),
                    depends_on_issue_id: a.id.clone(),
                    actor: None
                }
            )
            .is_err());
        assert!(service
            .add_dependency(
                &project,
                IssueDependencyInput {
                    issue_id: a.id.clone(),
                    depends_on_issue_id: b.id.clone(),
                    actor: None
                }
            )
            .is_err());
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn ready_excludes_unresolved_dependencies() {
        let service = IssueService::new();
        let project = temp_project("ready");
        let epic = create_test_epic(&service, &project);
        let a = service
            .create(
                &project,
                IssueCreateInput {
                    title: "A".into(),
                    description: None,
                    status: None,
                    priority: Some(1),
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        let b = service
            .create(
                &project,
                IssueCreateInput {
                    title: "B".into(),
                    description: None,
                    status: None,
                    priority: Some(0),
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        service
            .add_dependency(
                &project,
                IssueDependencyInput {
                    issue_id: b.id.clone(),
                    depends_on_issue_id: a.id.clone(),
                    actor: None,
                },
            )
            .unwrap();
        let ready = service
            .ready(&project, IssueReadyFilters::default())
            .unwrap();
        assert_eq!(
            ready.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec![a.id.as_str()]
        );
        service
            .update(
                &project,
                &a.id,
                IssueUpdateInput {
                    status: Some("completed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let ready = service
            .ready(&project, IssueReadyFilters::default())
            .unwrap();
        assert_eq!(
            ready.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            vec![b.id.as_str()]
        );
        let _ = std::fs::remove_dir_all(project);
    }

    #[test]
    fn search_escapes_like_wildcards() {
        let service = IssueService::new();
        let project = temp_project("search");
        let epic = create_test_epic(&service, &project);
        let literal = service
            .create(
                &project,
                IssueCreateInput {
                    title: "Use 100% CPU".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        service
            .create(
                &project,
                IssueCreateInput {
                    title: "Use 1000 CPU".into(),
                    description: None,
                    status: None,
                    priority: None,
                    epic_id: Some(epic.id.clone()),
                    parent_issue_id: None,
                    assignee_kind: None,
                    assignee_id: None,
                    tags: vec![],
                    context_json: None,
                    actor: None,
                },
            )
            .unwrap();
        let results = service
            .search(&project, "100%", IssueSearchFilters::default())
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, literal.id);
        let _ = std::fs::remove_dir_all(project);
    }
}
