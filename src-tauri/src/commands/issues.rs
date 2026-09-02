//! Tauri command surface for the folder-local issue store.
//!
//! Each handler is a thin async wrapper that canonicalizes the folder
//! path, then runs the matching [`IssueService`] call on a
//! `tokio::task::spawn_blocking` worker so the rusqlite call stays off
//! the Tauri runtime thread. Service errors are classified into
//! [`ApiError`] variants by [`map_issue_error`] so the frontend can
//! distinguish not-found from validation from infrastructure failures.

use std::sync::Arc;

use crate::app_state::AppState;
use crate::errors::ApiError;
use crate::models::issues::*;
use crate::services::folders::resolve_folder;

#[tauri::command]
pub async fn issues_list(
    folder_path: String,
    filters: IssueListFilters,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list(&folder, filters)).await
}

#[tauri::command]
pub async fn issues_ready(
    folder_path: String,
    limit: Option<i64>,
    filters: Option<IssueReadyFilters>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    let mut filters = filters.unwrap_or_default();
    if filters.limit.is_none() {
        filters.limit = limit;
    }
    run_blocking(move || issues.ready(&folder, filters)).await
}

#[tauri::command]
pub async fn issues_search(
    folder_path: String,
    query: String,
    filters: IssueSearchFilters,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.search(&folder, &query, filters)).await
}

#[tauri::command]
pub async fn issues_get(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueDetail, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    let lookup_id = issue_id.clone();
    run_blocking(move || {
        issues
            .get(&folder, &issue_id)
            .and_then(|item| item.ok_or_else(|| format!("Issue not found: {}", lookup_id)))
    })
    .await
}

#[tauri::command]
pub async fn issues_create(
    folder_path: String,
    input: IssueCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<Issue, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.create(&folder, input)).await
}

#[tauri::command]
pub async fn issues_update(
    folder_path: String,
    issue_id: String,
    patch: IssueUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<Issue, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.update(&folder, &issue_id, patch)).await
}

#[tauri::command]
pub async fn issues_delete(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.delete(&folder, &issue_id)).await
}

#[tauri::command]
pub async fn issue_epics_create(
    folder_path: String,
    input: IssueEpicCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.create_epic(&folder, input)).await
}

#[tauri::command]
pub async fn issue_epics_list(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueEpic>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_epics(&folder)).await
}

#[tauri::command]
pub async fn issue_epics_get(
    folder_path: String,
    epic_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    let lookup_id = epic_id.clone();
    run_blocking(move || {
        issues
            .get_epic(&folder, &epic_id)
            .and_then(|item| item.ok_or_else(|| format!("Epic not found: {}", lookup_id)))
    })
    .await
}

#[tauri::command]
pub async fn issue_epics_update(
    folder_path: String,
    epic_id: String,
    patch: IssueEpicUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.update_epic(&folder, &epic_id, patch)).await
}

#[tauri::command]
pub async fn issue_epics_delete(
    folder_path: String,
    epic_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.delete_epic(&folder, &epic_id)).await
}

#[tauri::command]
pub async fn issue_comments_add(
    folder_path: String,
    input: IssueCommentCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueComment, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.add_comment(&folder, input)).await
}

#[tauri::command]
pub async fn issue_comments_list(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueComment>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_comments(&folder, &issue_id)).await
}

#[tauri::command]
pub async fn issue_comments_update(
    folder_path: String,
    comment_id: String,
    input: IssueCommentUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueComment, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.update_comment(&folder, &comment_id, input)).await
}

#[tauri::command]
pub async fn issue_comments_delete(
    folder_path: String,
    comment_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.delete_comment(&folder, &comment_id)).await
}

#[tauri::command]
pub async fn issue_dependencies_add(
    folder_path: String,
    input: IssueDependencyInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueDependency, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.add_dependency(&folder, input)).await
}

#[tauri::command]
pub async fn issue_dependencies_remove(
    folder_path: String,
    input: IssueDependencyInput,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.remove_dependency(&folder, input)).await
}

#[tauri::command]
pub async fn issue_dependencies_list(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueDependency>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_dependencies(&folder, &issue_id)).await
}

#[tauri::command]
pub async fn issue_current_git_user(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    Ok(state
        .git
        .current_user_identity(&folder)
        .unwrap_or_else(|| "human".to_string()))
}

#[tauri::command]
pub async fn issue_config_list(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueConfigEntry>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.list_config(&folder)).await
}

#[tauri::command]
pub async fn issue_config_get(
    folder_path: String,
    key: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<IssueConfigEntry>, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.get_config(&folder, &key)).await
}

#[tauri::command]
pub async fn issue_config_set(
    folder_path: String,
    key: String,
    value: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueConfigEntry, ApiError> {
    let folder = resolve_folder(&folder_path)?;
    let issues = Arc::clone(&state.issues);
    run_blocking(move || issues.set_config(&folder, &key, &value)).await
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
