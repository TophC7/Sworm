use crate::app_state::AppState;
use sworm_core::errors::ApiError;
use sworm_protocol::branch::{BranchOpState, BranchSummary};
use sworm_protocol::file_diff::{DiffSource, FileDiff, GitStatus};
use sworm_protocol::git::{
    CommitDetail, DiffFileContent, GitQuickDiffData, GitSummary, GraphCommit, StashEntry,
};

#[tauri::command]
pub async fn git_get_summary(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<GitSummary, ApiError> {
    state.router.git_get_summary(path).await
}

#[tauri::command]
pub async fn git_watch(
    project_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    let windows = std::sync::Arc::clone(&state.windows);
    state
        .host
        .git_watch(project_path, move |folder| windows.folder_claimed(folder))
        .await
}

#[tauri::command]
pub async fn git_get_commit_detail(
    path: String,
    hash: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<CommitDetail>, ApiError> {
    state.host.git_get_commit_detail(path, hash).await
}

#[tauri::command]
pub async fn diff_get_files(
    path: String,
    source: DiffSource,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileDiff>, ApiError> {
    state.host.diff_get_files(path, source).await
}

#[tauri::command]
pub async fn diff_get_working_index(
    path: String,
    staged: bool,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileDiff>, ApiError> {
    state.host.diff_get_working_index(path, staged).await
}

#[tauri::command]
pub async fn diff_get_working_file(
    path: String,
    file_path: String,
    status: GitStatus,
    staged: bool,
    state: tauri::State<'_, AppState>,
) -> Result<DiffFileContent, ApiError> {
    state
        .host
        .diff_get_working_file(path, file_path, status, staged)
        .await
}

#[tauri::command]
pub async fn git_get_graph(
    path: String,
    limit: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GraphCommit>, ApiError> {
    state.host.git_get_graph(path, limit).await
}

#[tauri::command]
pub async fn git_get_branch_commits(
    path: String,
    branch: String,
    limit: usize,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<GraphCommit>, ApiError> {
    state.host.git_get_branch_commits(path, branch, limit).await
}

#[tauri::command]
pub async fn git_stage_all(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_stage_all(path).await
}

#[tauri::command]
pub async fn git_stage_files(
    path: String,
    files: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_stage_files(path, files).await
}

#[tauri::command]
pub async fn git_unstage_all(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_unstage_all(path).await
}

#[tauri::command]
pub async fn git_unstage_files(
    path: String,
    files: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_unstage_files(path, files).await
}

#[tauri::command]
pub async fn git_discard_all(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_discard_all(path).await
}

#[tauri::command]
pub async fn git_discard_files(
    path: String,
    files: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_discard_files(path, files).await
}

#[tauri::command]
pub async fn git_get_full_patch(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    state.host.git_get_full_patch(path).await
}

#[tauri::command]
pub async fn git_get_path_patch(
    path: String,
    files: Vec<String>,
    staged: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, ApiError> {
    state.host.git_get_path_patch(path, files, staged).await
}

#[tauri::command]
pub async fn git_get_quick_diff_data(
    project_path: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<GitQuickDiffData, ApiError> {
    state
        .host
        .git_get_quick_diff_data(project_path, file_path)
        .await
}

#[tauri::command]
pub async fn git_stage_file_content(
    project_path: String,
    file_path: String,
    content: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .git_stage_file_content(project_path, file_path, content)
        .await
}

#[tauri::command]
pub async fn git_commit(
    path: String,
    message: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.host.git_commit(path, message).await
}

#[tauri::command]
pub async fn git_undo_last_commit(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state.host.git_undo_last_commit(path).await
}

#[tauri::command]
pub async fn git_push(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.host.git_push(path).await
}

#[tauri::command]
pub async fn git_push_force_with_lease(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_push_force_with_lease(path).await
}

#[tauri::command]
pub async fn git_pull(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.host.git_pull(path).await
}

#[tauri::command]
pub async fn git_fetch(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.host.git_fetch(path).await
}

#[tauri::command]
pub async fn git_stash_all(
    path: String,
    message: Option<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_stash_all(path, message).await
}

#[tauri::command]
pub async fn git_stash_count(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<usize, ApiError> {
    state.host.git_stash_count(path).await
}

#[tauri::command]
pub async fn git_stash_list(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<StashEntry>, ApiError> {
    state.host.git_stash_list(path).await
}

#[tauri::command]
pub async fn git_stash_pop(
    path: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_stash_pop(path, index).await
}

#[tauri::command]
pub async fn git_stash_drop(
    path: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_stash_drop(path, index).await
}

#[tauri::command]
pub async fn git_show_file(
    project_path: String,
    git_ref: String,
    file_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<String, ApiError> {
    state
        .host
        .git_show_file(project_path, git_ref, file_path)
        .await
}

#[tauri::command]
pub async fn git_init(path: String, state: tauri::State<'_, AppState>) -> Result<(), ApiError> {
    state.host.git_init(path).await
}

#[tauri::command]
pub async fn git_clone_in_place(
    path: String,
    url: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_clone_in_place(path, url).await
}

#[tauri::command]
pub async fn git_list_branches(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<BranchSummary>, ApiError> {
    state.host.git_list_branches(path).await
}

#[tauri::command]
pub async fn git_branch_status(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<BranchOpState, ApiError> {
    state.host.git_branch_status(path).await
}

#[tauri::command]
pub async fn git_diff_branch_against_head(
    path: String,
    branch: String,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<FileDiff>, ApiError> {
    state.host.git_diff_branch_against_head(path, branch).await
}

#[tauri::command]
pub async fn git_checkout_branch(
    path: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_checkout_branch(path, name).await
}

#[tauri::command]
pub async fn git_checkout_remote_as_local(
    path: String,
    remote_name: String,
    local_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .git_checkout_remote_as_local(path, remote_name, local_name)
        .await
}

#[tauri::command]
pub async fn git_create_branch(
    path: String,
    name: String,
    base: String,
    checkout: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .git_create_branch(path, name, base, checkout)
        .await
}

#[tauri::command]
pub async fn git_rename_branch(
    path: String,
    old_name: String,
    new_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_rename_branch(path, old_name, new_name).await
}

#[tauri::command]
pub async fn git_delete_branch(
    path: String,
    name: String,
    force: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_delete_branch(path, name, force).await
}

#[tauri::command]
pub async fn git_delete_remote_branch(
    path: String,
    remote: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state
        .host
        .git_delete_remote_branch(path, remote, name)
        .await
}

#[tauri::command]
pub async fn git_set_upstream(
    path: String,
    branch: String,
    upstream: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_set_upstream(path, branch, upstream).await
}

#[tauri::command]
pub async fn git_fast_forward_branch(
    path: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_fast_forward_branch(path, name).await
}

#[tauri::command]
pub async fn git_merge_into_current(
    path: String,
    source: String,
    no_ff: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_merge_into_current(path, source, no_ff).await
}

#[tauri::command]
pub async fn git_rebase_current_onto(
    path: String,
    target: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_rebase_current_onto(path, target).await
}

#[tauri::command]
pub async fn git_rebase_continue(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_rebase_continue(path).await
}

#[tauri::command]
pub async fn git_rebase_skip(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_rebase_skip(path).await
}

#[tauri::command]
pub async fn git_rebase_abort(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_rebase_abort(path).await
}

#[tauri::command]
pub async fn git_merge_abort(
    path: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), ApiError> {
    state.host.git_merge_abort(path).await
}
