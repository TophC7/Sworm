//! Tauri command surface for the project-local issue store.
//!
//! Each handler is a thin async wrapper that resolves the project path
//! from the global DB, then runs the matching [`IssueService`] call on a
//! `tokio::task::spawn_blocking` worker so the rusqlite call stays off
//! the Tauri runtime thread. Service errors are classified into
//! [`ApiError`] variants by [`map_issue_error`] so the frontend can
//! distinguish not-found from validation from infrastructure failures.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::issues::*;

#[tauri::command]
pub async fn issues_list(
    project_id: String,
    filters: IssueListFilters,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list(&project_path, filters)).await
}

#[tauri::command]
pub async fn issues_ready(
    project_id: String,
    limit: Option<i64>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.ready(&project_path, limit)).await
}

#[tauri::command]
pub async fn issues_search(
    project_id: String,
    query: String,
    filters: IssueSearchFilters,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.search(&project_path, &query, filters)).await
}

#[tauri::command]
pub async fn issues_get(
    project_id: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueDetail, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    let lookup_id = issue_id.clone();
    run_blocking(move || {
        issues
            .get(&project_path, &issue_id)
            .and_then(|item| item.ok_or_else(|| format!("Issue not found: {}", lookup_id)))
    })
    .await
}

#[tauri::command]
pub async fn issues_create(
    project_id: String,
    input: IssueCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<Issue, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.create(&project_path, input)).await
}

#[tauri::command]
pub async fn issues_update(
    project_id: String,
    issue_id: String,
    patch: IssueUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<Issue, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.update(&project_path, &issue_id, patch)).await
}

#[tauri::command]
pub async fn issues_delete(
    project_id: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.delete(&project_path, &issue_id)).await
}

#[tauri::command]
pub async fn issue_epics_create(
    project_id: String,
    input: IssueEpicCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.create_epic(&project_path, input)).await
}

#[tauri::command]
pub async fn issue_epics_list(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueEpic>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_epics(&project_path)).await
}

#[tauri::command]
pub async fn issue_epics_get(
    project_id: String,
    epic_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    let lookup_id = epic_id.clone();
    run_blocking(move || {
        issues
            .get_epic(&project_path, &epic_id)
            .and_then(|item| item.ok_or_else(|| format!("Epic not found: {}", lookup_id)))
    })
    .await
}

#[tauri::command]
pub async fn issue_epics_update(
    project_id: String,
    epic_id: String,
    patch: IssueEpicUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.update_epic(&project_path, &epic_id, patch)).await
}

#[tauri::command]
pub async fn issue_epics_delete(
    project_id: String,
    epic_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.delete_epic(&project_path, &epic_id)).await
}

#[tauri::command]
pub async fn issue_comments_add(
    project_id: String,
    input: IssueCommentCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueComment, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.add_comment(&project_path, input)).await
}

#[tauri::command]
pub async fn issue_comments_list(
    project_id: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueComment>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_comments(&project_path, &issue_id)).await
}

#[tauri::command]
pub async fn issue_comments_update(
    project_id: String,
    comment_id: String,
    input: IssueCommentUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueComment, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.update_comment(&project_path, &comment_id, input)).await
}

#[tauri::command]
pub async fn issue_comments_delete(
    project_id: String,
    comment_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.delete_comment(&project_path, &comment_id)).await
}

#[tauri::command]
pub async fn issue_dependencies_add(
    project_id: String,
    input: IssueDependencyInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueDependency, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.add_dependency(&project_path, input)).await
}

#[tauri::command]
pub async fn issue_dependencies_remove(
    project_id: String,
    input: IssueDependencyInput,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.remove_dependency(&project_path, input)).await
}

#[tauri::command]
pub async fn issue_dependencies_list(
    project_id: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueDependency>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_dependencies(&project_path, &issue_id)).await
}

#[tauri::command]
pub async fn issue_current_git_user(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    Ok(state
        .git
        .current_user_identity(&project_path)
        .unwrap_or_else(|| "human".to_string()))
}

#[tauri::command]
pub async fn issue_config_list(
    project_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueConfigEntry>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_config(&project_path)).await
}

#[tauri::command]
pub async fn issue_config_get(
    project_id: String,
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<IssueConfigEntry>, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.get_config(&project_path, &key)).await
}

#[tauri::command]
pub async fn issue_config_set(
    project_id: String,
    key: String,
    value: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueConfigEntry, ApiError> {
    let project_path = project_path_for(&project_id, &state)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.set_config(&project_path, &key, &value)).await
}

/// Run an [`IssueService`] call on a blocking worker and translate
/// errors into the typed [`ApiError`] variants the frontend matches on.
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

/// Classify a service-layer string error into an [`ApiError`] variant.
/// Substring match because [`IssueService`] is intentionally
/// stringly-typed today; the patterns below cover every validation /
/// not-found / conflict message it produces.
pub(crate) fn map_issue_error(message: String) -> ApiError {
    if is_not_found(&message) {
        ApiError::NotFound(message)
    } else if is_validation(&message) {
        ApiError::InvalidArgument(message)
    } else {
        ApiError::Database(message)
    }
}

fn is_not_found(message: &str) -> bool {
    message.contains("not found") || message.contains("not Found") || message.contains("Not found")
}

fn is_validation(message: &str) -> bool {
    const PATTERNS: &[&str] = &[
        "must not be empty",
        "must be between",
        "Invalid issue status",
        "Invalid epic status",
        "Invalid assignee kind",
        "assigneeId required",
        "Invalid issue config key",
        "Prefix must",
        "Sub-issues are one level deep",
        "must belong to an epic",
        "Sub-issue epic must match",
        "cannot depend on itself",
        "already exists",
        "would create a cycle",
        "Cannot delete epic while it has issues",
        "Tags must not be empty",
        "Issue must belong",
        "Value must not",
    ];
    PATTERNS.iter().any(|p| message.contains(p))
}

fn project_path_for(
    project_id: &str,
    state: &tauri::State<'_, AppState>,
) -> Result<PathBuf, ApiError> {
    let db = state.db.read();
    let project = state
        .projects
        .get(db.conn(), project_id)
        .map_err(ApiError::Database)?
        .ok_or_else(|| ApiError::NotFound(format!("Project not found: {}", project_id)))?;
    Ok(Path::new(&project.path).to_path_buf())
}
