//! Folder-local issue workflows.

use crate::{errors::ApiError, events::HostEvent, services::folders::resolve_folder, Host};
use std::{path::Path, sync::Arc};
use sworm_protocol::issues::*;

impl Host {
    pub async fn issues_list(
        &self,
        folder_path: String,
        filters: IssueListFilters,
    ) -> Result<Vec<Issue>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        run_blocking(move || issues.list(&folder, filters)).await
    }

    pub async fn issues_ready(
        &self,
        folder_path: String,
        limit: Option<i64>,
        filters: Option<IssueReadyFilters>,
    ) -> Result<Vec<Issue>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        let mut filters = filters.unwrap_or_default();
        if filters.limit.is_none() {
            filters.limit = limit;
        }
        run_blocking(move || issues.ready(&folder, filters)).await
    }

    pub async fn issues_search(
        &self,
        folder_path: String,
        query: String,
        filters: IssueSearchFilters,
    ) -> Result<Vec<Issue>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        run_blocking(move || issues.search(&folder, &query, filters)).await
    }

    pub async fn issues_get(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<IssueDetail, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        let lookup_id = issue_id.clone();
        run_blocking(move || {
            issues
                .get(&folder, &issue_id)
                .and_then(|item| item.ok_or_else(|| format!("Issue not found: {}", lookup_id)))
        })
        .await
    }

    pub async fn issues_create(
        &self,
        folder_path: String,
        input: IssueCreateInput,
    ) -> Result<Issue, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| issues.create(folder, input))
            .await
    }

    pub async fn issues_update(
        &self,
        folder_path: String,
        issue_id: String,
        patch: IssueUpdateInput,
    ) -> Result<Issue, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| {
            issues.update(folder, &issue_id, patch)
        })
        .await
    }

    pub async fn issues_delete(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| issues.delete(folder, &issue_id))
            .await
    }

    pub async fn issue_epics_create(
        &self,
        folder_path: String,
        input: IssueEpicCreateInput,
    ) -> Result<IssueEpic, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| issues.create_epic(folder, input))
            .await
    }

    pub async fn issue_epics_list(&self, folder_path: String) -> Result<Vec<IssueEpic>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        run_blocking(move || issues.list_epics(&folder)).await
    }

    pub async fn issue_epics_get(
        &self,
        folder_path: String,
        epic_id: String,
    ) -> Result<IssueEpic, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        let lookup_id = epic_id.clone();
        run_blocking(move || {
            issues
                .get_epic(&folder, &epic_id)
                .and_then(|item| item.ok_or_else(|| format!("Epic not found: {}", lookup_id)))
        })
        .await
    }

    pub async fn issue_epics_update(
        &self,
        folder_path: String,
        epic_id: String,
        patch: IssueEpicUpdateInput,
    ) -> Result<IssueEpic, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| {
            issues.update_epic(folder, &epic_id, patch)
        })
        .await
    }

    pub async fn issue_epics_delete(
        &self,
        folder_path: String,
        epic_id: String,
    ) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| issues.delete_epic(folder, &epic_id))
            .await
    }

    pub async fn issue_comments_add(
        &self,
        folder_path: String,
        input: IssueCommentCreateInput,
    ) -> Result<IssueComment, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| issues.add_comment(folder, input))
            .await
    }

    pub async fn issue_comments_list(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<Vec<IssueComment>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        run_blocking(move || issues.list_comments(&folder, &issue_id)).await
    }

    pub async fn issue_comments_update(
        &self,
        folder_path: String,
        comment_id: String,
        input: IssueCommentUpdateInput,
    ) -> Result<IssueComment, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| {
            issues.update_comment(folder, &comment_id, input)
        })
        .await
    }

    pub async fn issue_comments_delete(
        &self,
        folder_path: String,
        comment_id: String,
    ) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| {
            issues.delete_comment(folder, &comment_id)
        })
        .await
    }

    pub async fn issue_dependencies_add(
        &self,
        folder_path: String,
        input: IssueDependencyInput,
    ) -> Result<IssueDependency, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| issues.add_dependency(folder, input))
            .await
    }

    pub async fn issue_dependencies_remove(
        &self,
        folder_path: String,
        input: IssueDependencyInput,
    ) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        self.run_mutation(&folder, move |folder| {
            issues.remove_dependency(folder, input)
        })
        .await
    }

    pub async fn issue_dependencies_list(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<Vec<IssueDependency>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        run_blocking(move || issues.list_dependencies(&folder, &issue_id)).await
    }

    pub async fn issue_current_git_user(&self, folder_path: String) -> Result<String, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        Ok(self
            .git
            .current_user_identity(&folder)
            .unwrap_or_else(|| "human".to_string()))
    }

    pub async fn issue_config_list(
        &self,
        folder_path: String,
    ) -> Result<Vec<IssueConfigEntry>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = Arc::clone(&self.issues);
        run_blocking(move || issues.list_config(&folder)).await
    }

    async fn run_mutation<T, F>(&self, folder: &Path, work: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&Path) -> Result<T, String> + Send + 'static,
    {
        let folder_path = folder.to_string_lossy().into_owned();
        let work_folder = folder.to_path_buf();
        let result = run_blocking(move || work(&work_folder)).await?;
        (self.events)(HostEvent::IssuesChanged(folder_path)).map_err(ApiError::Internal)?;
        Ok(result)
    }
}

/// Run an issue service call on a blocking worker and preserve frontend error categories.
async fn run_blocking<T, F>(work: F) -> Result<T, ApiError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| ApiError::Internal(error.to_string()))?
        .map_err(map_issue_error)
}

pub(crate) fn map_issue_error(message: String) -> ApiError {
    match crate::services::issues::classify_issue_error(&message) {
        crate::services::issues::IssueErrorKind::NotFound => ApiError::NotFound(message),
        crate::services::issues::IssueErrorKind::Validation => ApiError::InvalidArgument(message),
        crate::services::issues::IssueErrorKind::Domain => ApiError::Database(message),
    }
}
