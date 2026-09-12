use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::issues::*;

#[tauri::command]
pub async fn issues_list(
    folder_path: String,
    filters: IssueListFilters,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    state.router.issues_list(folder_path, filters).await
}

#[tauri::command]
pub async fn issues_ready(
    folder_path: String,
    limit: Option<i64>,
    filters: Option<IssueReadyFilters>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    state.router.issues_ready(folder_path, limit, filters).await
}

#[tauri::command]
pub async fn issues_search(
    folder_path: String,
    query: String,
    filters: IssueSearchFilters,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Issue>, ApiError> {
    state
        .router
        .issues_search(folder_path, query, filters)
        .await
}

#[tauri::command]
pub async fn issues_get(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueDetail, ApiError> {
    state.router.issues_get(folder_path, issue_id).await
}

#[tauri::command]
pub async fn issues_create(
    folder_path: String,
    input: IssueCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<Issue, ApiError> {
    state.router.issues_create(folder_path, input).await
}

#[tauri::command]
pub async fn issues_update(
    folder_path: String,
    issue_id: String,
    patch: IssueUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<Issue, ApiError> {
    state
        .router
        .issues_update(folder_path, issue_id, patch)
        .await
}

#[tauri::command]
pub async fn issues_delete(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.router.issues_delete(folder_path, issue_id).await
}

#[tauri::command]
pub async fn issue_epics_create(
    folder_path: String,
    input: IssueEpicCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    state.router.issue_epics_create(folder_path, input).await
}

#[tauri::command]
pub async fn issue_epics_list(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueEpic>, ApiError> {
    state.router.issue_epics_list(folder_path).await
}

#[tauri::command]
pub async fn issue_epics_get(
    folder_path: String,
    epic_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    state.router.issue_epics_get(folder_path, epic_id).await
}

#[tauri::command]
pub async fn issue_epics_update(
    folder_path: String,
    epic_id: String,
    patch: IssueEpicUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueEpic, ApiError> {
    state
        .router
        .issue_epics_update(folder_path, epic_id, patch)
        .await
}

#[tauri::command]
pub async fn issue_epics_delete(
    folder_path: String,
    epic_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.router.issue_epics_delete(folder_path, epic_id).await
}

#[tauri::command]
pub async fn issue_comments_add(
    folder_path: String,
    input: IssueCommentCreateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueComment, ApiError> {
    state.router.issue_comments_add(folder_path, input).await
}

#[tauri::command]
pub async fn issue_comments_list(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueComment>, ApiError> {
    state
        .router
        .issue_comments_list(folder_path, issue_id)
        .await
}

#[tauri::command]
pub async fn issue_comments_update(
    folder_path: String,
    comment_id: String,
    input: IssueCommentUpdateInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueComment, ApiError> {
    state
        .router
        .issue_comments_update(folder_path, comment_id, input)
        .await
}

#[tauri::command]
pub async fn issue_comments_delete(
    folder_path: String,
    comment_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .router
        .issue_comments_delete(folder_path, comment_id)
        .await
}

#[tauri::command]
pub async fn issue_dependencies_add(
    folder_path: String,
    input: IssueDependencyInput,
    state: tauri::State<'_, AppState>,
) -> Result<IssueDependency, ApiError> {
    state
        .router
        .issue_dependencies_add(folder_path, input)
        .await
}

#[tauri::command]
pub async fn issue_dependencies_remove(
    folder_path: String,
    input: IssueDependencyInput,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .router
        .issue_dependencies_remove(folder_path, input)
        .await
}

#[tauri::command]
pub async fn issue_dependencies_list(
    folder_path: String,
    issue_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueDependency>, ApiError> {
    state
        .router
        .issue_dependencies_list(folder_path, issue_id)
        .await
}

#[tauri::command]
pub async fn issue_current_git_user(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.router.issue_current_git_user(folder_path).await
}

#[tauri::command]
pub async fn issue_config_list(
    folder_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<IssueConfigEntry>, ApiError> {
    state.router.issue_config_list(folder_path).await
}
