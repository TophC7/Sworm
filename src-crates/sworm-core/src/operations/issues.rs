//! Folder-local issue workflows.

use crate::{errors::ApiError, events::HostEvent, services::folders::resolve_folder, Host};
use std::path::Path;
use sworm_protocol::issues::*;

impl Host {
    pub fn issues_list(
        &self,
        folder_path: String,
        filters: IssueListFilters,
    ) -> Result<Vec<Issue>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.list(&folder, filters)
    }

    pub fn issues_ready(
        &self,
        folder_path: String,
        filters: IssueReadyFilters,
    ) -> Result<Vec<Issue>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.ready(&folder, filters)
    }

    pub fn issues_search(
        &self,
        folder_path: String,
        query: String,
        filters: IssueListFilters,
    ) -> Result<Vec<Issue>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.search(&folder, &query, filters)
    }

    pub fn issues_get(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<IssueDetail, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        self.issues.get(&folder, &issue_id)
    }

    pub fn issues_create(
        &self,
        folder_path: String,
        input: IssueCreateInput,
    ) -> Result<Issue, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| issues.create(folder, input))
    }

    pub fn issues_update(
        &self,
        folder_path: String,
        issue_id: String,
        patch: IssueUpdateInput,
    ) -> Result<Issue, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| {
            issues.update(folder, &issue_id, patch)
        })
    }

    pub fn issues_delete(&self, folder_path: String, issue_id: String) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| issues.delete(folder, &issue_id))
    }

    pub fn issue_epics_create(
        &self,
        folder_path: String,
        input: IssueEpicCreateInput,
    ) -> Result<IssueEpic, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| issues.create_epic(folder, input))
    }

    pub fn issue_epics_list(&self, folder_path: String) -> Result<Vec<IssueEpic>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.list_epics(&folder)
    }

    pub fn issue_epics_get(
        &self,
        folder_path: String,
        epic_id: String,
    ) -> Result<IssueEpic, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        self.issues.get_epic(&folder, &epic_id)
    }

    pub fn issue_epics_update(
        &self,
        folder_path: String,
        epic_id: String,
        patch: IssueEpicUpdateInput,
    ) -> Result<IssueEpic, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| {
            issues.update_epic(folder, &epic_id, patch)
        })
    }

    pub fn issue_epics_delete(&self, folder_path: String, epic_id: String) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| issues.delete_epic(folder, &epic_id))
    }

    pub fn issue_comments_add(
        &self,
        folder_path: String,
        input: IssueCommentCreateInput,
    ) -> Result<IssueComment, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| issues.add_comment(folder, input))
    }

    pub fn issue_comments_list(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<Vec<IssueComment>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.list_comments(&folder, &issue_id)
    }

    pub fn issue_comments_update(
        &self,
        folder_path: String,
        comment_id: String,
        input: IssueCommentUpdateInput,
    ) -> Result<IssueComment, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| {
            issues.update_comment(folder, &comment_id, input)
        })
    }

    pub fn issue_comments_delete(
        &self,
        folder_path: String,
        comment_id: String,
    ) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| {
            issues.delete_comment(folder, &comment_id)
        })
    }

    pub fn issue_dependencies_add(
        &self,
        folder_path: String,
        input: IssueDependencyInput,
    ) -> Result<IssueDependency, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| issues.add_dependency(folder, input))
    }

    pub fn issue_dependencies_remove(
        &self,
        folder_path: String,
        input: IssueDependencyInput,
    ) -> Result<(), ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        self.run_mutation(&folder, move |folder| {
            issues.remove_dependency(folder, input)
        })
    }

    pub fn issue_dependencies_list(
        &self,
        folder_path: String,
        issue_id: String,
    ) -> Result<Vec<IssueDependency>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.list_dependencies(&folder, &issue_id)
    }

    pub fn issue_current_git_user(&self, folder_path: String) -> Result<String, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        Ok(self
            .git
            .current_user_identity(&folder)
            .unwrap_or_else(|| "human".to_string()))
    }

    pub fn issue_config_list(
        &self,
        folder_path: String,
    ) -> Result<Vec<IssueConfigEntry>, ApiError> {
        let folder = resolve_folder(&folder_path)?;
        let issues = &self.issues;
        issues.list_config(&folder)
    }

    fn run_mutation<T>(
        &self,
        folder: &Path,
        work: impl FnOnce(&Path) -> Result<T, ApiError>,
    ) -> Result<T, ApiError> {
        let result = work(folder)?;
        self.emit(HostEvent::IssuesChanged(
            folder.to_string_lossy().into_owned(),
        ));
        Ok(result)
    }
}
