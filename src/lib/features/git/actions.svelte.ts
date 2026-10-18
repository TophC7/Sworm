import { backend } from '$lib/api/backend'
import { revealGitChanges } from '$lib/features/app-shell/sidebar/state.svelte'
import { confirmAsync, pickAsync, promptAsync } from '$lib/features/confirm/service.svelte'
import {
  getCommitState,
  getEditedCommitMessage,
  setCommitDraft,
  setSkipHooks,
  syncCommitTemplate
} from '$lib/features/git/commit.svelte'
import type { PostCommit } from '$lib/features/git/commitButton'
import { isGitError } from '$lib/features/git/errors'
import { describeOperation, type OperationDisplay } from '$lib/features/git/operation'
import {
  discardAll,
  getGitSummary,
  runGitAction,
  stageAll,
  stageFiles,
  unstageAll
} from '$lib/features/git/state.svelte'
import { runNotifiedTask, type RunNotifiedTaskOptions } from '$lib/features/notifications/runNotifiedTask'
import { notify } from '$lib/features/notifications/state.svelte'
import { forgetRecentWorktree } from '$lib/features/folders/state.svelte'
import { forgetWorktree, refreshWorktreeRepository, refreshWorktrees } from '$lib/features/git/worktrees.svelte'
import { openTaskTab, startTaskTab } from '$lib/features/tasks/service.svelte'
import { closeWorktreeTabs } from '$lib/features/workbench/tabActions.svelte'
import {
  forgetClosedWorktreeTabs,
  getActiveFolderPath,
  getTabs,
  openFolder
} from '$lib/features/workbench/state.svelte'
import type {
  GitConflict,
  GitConflictSide,
  GitRemote,
  GitResetMode,
  GitSequencerAction,
  GitStashScope,
  GitWorktreeCheckout,
  GitWorktreeResult
} from '$lib/types/backend'
import { getErrorMessage } from '$lib/utils/client-error'
import { basename, isPathWithin } from '$lib/utils/paths'
import { SvelteSet } from 'svelte/reactivity'
import { platform } from '$lib/platform'

const GIT_ACTION_NOTIFICATIONS = {
  pull: {
    loading: { title: 'Pulling changes' },
    success: { title: 'Pull complete' },
    error: { title: 'Pull failed' }
  },
  push: {
    loading: { title: 'Pushing changes' },
    success: { title: 'Push complete' },
    error: { title: 'Push failed' }
  },
  forcePush: {
    loading: { title: 'Force pushing changes' },
    success: { title: 'Force push complete' },
    error: { title: 'Force push failed' }
  },
  fetch: {
    loading: { title: 'Fetching remote changes' },
    success: { title: 'Fetch complete' },
    error: { title: 'Fetch failed' }
  },
  publish: {
    loading: { title: 'Publishing branch' },
    success: { title: 'Branch published' },
    error: { title: 'Publish failed' }
  },
  sync: {
    loading: { title: 'Syncing changes' },
    success: { title: 'Sync complete' },
    error: { title: 'Sync failed' }
  },
  commit: {
    loading: { title: 'Creating commit' },
    success: { title: 'Commit created', description: (hash: string) => hash.slice(0, 7) },
    error: { title: 'Commit failed' }
  },
  undoLastCommit: {
    loading: { title: 'Undoing last commit' },
    success: { title: 'Commit undone' },
    error: { title: 'Undo commit failed' }
  },
  stageAll: {
    loading: { title: 'Staging all changes' },
    success: { title: (ran: boolean) => (ran ? 'All changes staged' : 'Stage cancelled') },
    error: { title: 'Stage all failed' }
  },
  unstageAll: {
    loading: { title: 'Unstaging all changes' },
    success: { title: 'All changes unstaged' },
    error: { title: 'Unstage all failed' }
  },
  discardAll: {
    loading: { title: 'Discarding all changes' },
    success: { title: (ran: boolean) => (ran ? 'All changes discarded' : 'Discard cancelled') },
    error: { title: 'Discard all failed' }
  },
  stash: {
    loading: { title: 'Stashing changes' },
    success: { title: 'Changes stashed' },
    error: { title: 'Stash failed' }
  },
  applyStash: {
    loading: { title: 'Applying stash' },
    success: { title: 'Stash applied' },
    error: { title: 'Apply stash failed' }
  },
  dropStash: {
    loading: { title: 'Dropping stash' },
    success: { title: 'Stash dropped' },
    error: { title: 'Drop stash failed' }
  },
  clearStashes: {
    loading: { title: 'Dropping all stashes' },
    success: { title: 'All stashes dropped' },
    error: { title: 'Drop stashes failed' }
  },
  branchFromStash: {
    loading: { title: 'Creating branch' },
    success: { title: 'Branch created' },
    error: { title: 'Create branch failed' }
  },
  ignore: {
    loading: { title: 'Adding to .gitignore' },
    success: { title: 'Added to .gitignore' },
    error: { title: 'Add to .gitignore failed' }
  },
  fastForward: {
    loading: { title: 'Fast-forwarding branch' },
    success: { title: 'Branch fast-forwarded' },
    error: { title: 'Fast-forward failed' }
  },
  rebase: {
    loading: { title: 'Rebasing' },
    success: { title: 'Rebase complete' },
    error: { title: 'Rebase failed' }
  },
  unsetUpstream: {
    loading: { title: 'Unsetting upstream' },
    success: { title: 'Upstream unset' },
    error: { title: 'Unset upstream failed' }
  },
  cherryPick: {
    loading: { title: 'Cherry-picking commit' },
    success: { title: 'Commit cherry-picked' },
    error: { title: 'Cherry-pick failed' }
  },
  revert: {
    loading: { title: 'Reverting commit' },
    success: { title: 'Commit reverted' },
    error: { title: 'Revert failed' }
  },
  reset: {
    loading: { title: 'Resetting branch' },
    success: { title: 'Reset complete' },
    error: { title: 'Reset failed' }
  },
  createTag: {
    loading: { title: 'Creating tag' },
    success: { title: 'Tag created' },
    error: { title: 'Create tag failed' }
  },
  deleteTag: {
    loading: { title: 'Deleting tag' },
    success: { title: 'Tag deleted' },
    error: { title: 'Delete tag failed' }
  },
  pushTags: {
    loading: { title: 'Pushing tags' },
    success: { title: 'Tags pushed' },
    error: { title: 'Push tags failed' }
  },
  deleteRemoteTag: {
    loading: { title: 'Deleting remote tag' },
    success: { title: 'Remote tag deleted' },
    error: { title: 'Delete remote tag failed' }
  },
  addRemote: {
    loading: { title: 'Adding remote' },
    success: { title: 'Remote added' },
    error: { title: 'Add remote failed' }
  },
  removeRemote: {
    loading: { title: 'Removing remote' },
    success: { title: 'Remote removed' },
    error: { title: 'Remove remote failed' }
  },
  renameRemote: {
    loading: { title: 'Renaming remote' },
    success: { title: 'Remote renamed' },
    error: { title: 'Rename remote failed' }
  },
  setRemoteUrl: {
    loading: { title: 'Setting remote URL' },
    success: { title: 'Remote URL set' },
    error: { title: 'Set remote URL failed' }
  },
  deleteMergedBranches: {
    loading: { title: 'Deleting merged branches' },
    success: { title: 'Merged branches deleted' },
    error: { title: 'Delete merged branches failed' }
  },
  continueOperation: (display: OperationDisplay) => ({
    loading: { title: `Continuing ${display.noun}` },
    success: { title: `${display.title} continued` },
    error: { title: 'Continue failed' }
  }),
  skipOperation: {
    loading: { title: 'Skipping commit' },
    success: { title: 'Commit skipped' },
    error: { title: 'Skip failed' }
  },
  abortOperation: (display: OperationDisplay) => ({
    loading: { title: `Aborting ${display.noun}` },
    success: { title: `${display.title} aborted` },
    error: { title: 'Abort failed' }
  }),
  takeConflictSide: {
    loading: { title: 'Resolving conflicts' },
    success: { title: 'Conflicts resolved' },
    error: { title: 'Resolve failed' }
  }
}

function gitRecovery(folderPath: string, error: unknown): (() => Promise<unknown>) | null {
  if (isGitError(error, 'noUpstream')) {
    return async () => {
      const proceed = await confirmAsync({
        title: 'Publish Branch?',
        message: `The branch "${error.branch}" has no remote branch. Would you like to publish this branch?`,
        confirmLabel: 'Publish'
      })
      if (proceed) await publishBranch(folderPath, error.branch)
    }
  }
  if (isGitError(error, 'pushRejected')) {
    return async () => {
      const proceed = await confirmAsync({
        title: 'Push Rejected',
        message: 'Can\'t push refs to remote. Try running "Pull" first to integrate your changes.',
        confirmLabel: 'Pull'
      })
      if (proceed) await pullFolder(folderPath)
    }
  }
  if (isGitError(error, 'mergeConflicts')) {
    return async () => showMergeConflicts(error.message)
  }
  return null
}

export function showMergeConflicts(detail: string): void {
  revealGitChanges()
  notify.warning('Merge conflicts', `Resolve the conflicts in Source Control, then continue.\n${detail}`)
}

function runNotifiedGitAction<T>(
  folderPath: string,
  options: RunNotifiedTaskOptions<T>,
  task: () => Promise<T>
): Promise<T | undefined> {
  return runNotifiedTask(task, { ...options, recover: (error) => gitRecovery(folderPath, error) })
}

export function pullFolder(folderPath: string, rebase = false): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.pull, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'pull', rebase }))
  )
}

export function pushFolder(folderPath: string, branch: string | null = null): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.push, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'push', branch, force: false }))
  )
}

export async function forcePushWithLease(folderPath: string, branch: string | null = null): Promise<void> {
  const proceed = await confirmAsync({
    title: 'Force Push?',
    message:
      'This will push with --force-with-lease. Remote commits may be overwritten if your local branch is ahead of the remote.',
    confirmLabel: 'Force Push',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.forcePush, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'push', branch, force: true }))
  )
}

export function fetchFolder(folderPath: string, remote: string | null = null): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.fetch, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'fetch', remote }))
  )
}

async function pickRemote(
  folderPath: string,
  errorTitle: string,
  title: string,
  message: string
): Promise<GitRemote | null> {
  let remotes: GitRemote[]
  try {
    remotes = await backend.git.listRemotes(folderPath)
  } catch (error) {
    notify.error(errorTitle, getErrorMessage(error))
    return null
  }
  if (remotes.length === 0) {
    notify.info('No remotes', 'This repository has no remotes.')
    return null
  }
  if (remotes.length === 1) return remotes[0]
  const ordered = remotes.toSorted(
    (a, b) => Number(b.pushDefault) - Number(a.pushDefault) || Number(b.name === 'origin') - Number(a.name === 'origin')
  )
  const name = await pickAsync({
    title,
    message,
    options: ordered.map((remote) => ({
      value: remote.name,
      label: remote.name,
      description: remote.pushUrl ?? remote.fetchUrl ?? undefined
    }))
  })
  return remotes.find((remote) => remote.name === name) ?? null
}

export async function publishBranch(
  folderPath: string,
  branch: string | null = null,
  remote: string | null = null
): Promise<void | undefined> {
  if (remote === null) {
    const name = branch ?? getGitSummary(folderPath)?.branch ?? ''
    const chosen = await pickRemote(
      folderPath,
      'Publish failed',
      'Publish Branch',
      `Pick a remote to publish the branch "${name}" to:`
    )
    if (!chosen) return
    remote = chosen.name
  }
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.publish, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'publish', branch, remote }))
  )
}

export function cherryPickCommit(folderPath: string, rev: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.cherryPick, () =>
    runGitAction(folderPath, (path) => backend.git.history(path, { kind: 'cherry_pick', rev }))
  )
}

export function revertCommit(folderPath: string, rev: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.revert, () =>
    runGitAction(folderPath, (path) => backend.git.history(path, { kind: 'revert', rev }))
  )
}

export async function resetToCommit(
  folderPath: string,
  rev: string,
  shortHash: string,
  mode: GitResetMode
): Promise<void> {
  if (mode === 'hard') {
    const branch = getGitSummary(folderPath)?.branch
    const proceed = await confirmAsync({
      title: 'Reset Hard?',
      message: `Reset ${branch ?? 'HEAD'} to ${shortHash}? Uncommitted changes to tracked files are permanently discarded, and commits after ${shortHash} leave the branch.`,
      confirmLabel: 'Reset Hard'
    })
    if (!proceed) return
  }
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.reset, () =>
    runGitAction(folderPath, (path) => backend.git.history(path, { kind: 'reset', rev, mode }))
  )
}

export async function createTag(folderPath: string, rev: string, revLabel: string): Promise<void> {
  const answer = await promptAsync({
    title: 'Create Tag',
    message: `Tag ${revLabel}.`,
    placeholder: 'Tag name',
    confirmLabel: 'Next'
  })
  const name = answer?.trim()
  if (!name) return
  const message = await promptAsync({
    title: 'Create Tag',
    message: 'Optionally provide a message to annotate the tag.',
    placeholder: 'Message',
    confirmLabel: 'Create Tag'
  })
  if (message === null) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.createTag, () =>
    runGitAction(folderPath, (path) =>
      backend.git.tag(path, { kind: 'create', name, rev, message: message.trim() || null })
    )
  )
}

export async function deleteTag(folderPath: string, name: string): Promise<void> {
  const proceed = await confirmAsync({
    title: 'Delete Tag?',
    message: `Delete tag "${name}"? This permanently removes the tag reference from this repository.`,
    confirmLabel: 'Delete Tag'
  })
  if (!proceed) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.deleteTag, () =>
    runGitAction(folderPath, (path) => backend.git.tag(path, { kind: 'delete', name }))
  )
}

export async function pushTag(folderPath: string, name: string | null): Promise<void> {
  const remote = await pickRemote(
    folderPath,
    'Push tags failed',
    name === null ? 'Push All Tags' : 'Push Tag',
    name === null ? 'Pick a remote to push all tags to:' : `Pick a remote to push tag "${name}" to:`
  )
  if (!remote) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.pushTags, () =>
    runGitAction(folderPath, (path) => backend.git.tag(path, { kind: 'push', remote: remote.name, name }))
  )
}

export async function deleteRemoteTag(folderPath: string, name: string): Promise<void> {
  const remote = await pickRemote(
    folderPath,
    'Delete remote tag failed',
    'Delete Remote Tag',
    `Pick a remote to delete tag "${name}" from:`
  )
  if (!remote) return
  const proceed = await confirmAsync({
    title: 'Delete Remote Tag?',
    message: `Delete tag "${name}" from remote "${remote.name}"? Other clones keep their copy.`,
    confirmLabel: 'Delete Tag'
  })
  if (!proceed) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.deleteRemoteTag, () =>
    runGitAction(folderPath, (path) => backend.git.tag(path, { kind: 'delete_remote', remote: remote.name, name }))
  )
}

export async function addRemote(folderPath: string): Promise<void> {
  const urlAnswer = await promptAsync({
    title: 'Add Remote',
    placeholder: 'Repository URL',
    confirmLabel: 'Next'
  })
  const url = urlAnswer?.trim()
  if (!url) return
  let remotes: GitRemote[]
  try {
    remotes = await backend.git.listRemotes(folderPath)
  } catch (error) {
    notify.error('Add remote failed', getErrorMessage(error))
    return
  }
  const answer = await promptAsync({
    title: 'Add Remote',
    placeholder: 'Remote name',
    value: remotes.length === 0 ? 'origin' : '',
    confirmLabel: 'Add Remote'
  })
  const name = answer?.trim()
  if (!name) return
  if (remotes.some((remote) => remote.name === name)) {
    notify.error('Add remote failed', `Remote "${name}" already exists.`)
    return
  }
  const added = await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.addRemote, () =>
    runGitAction(folderPath, (path) => backend.git.remoteConfig(path, { kind: 'add', name, url })).then(() => true)
  )
  if (added) await fetchFolder(folderPath, name)
}

export async function removeRemote(folderPath: string): Promise<void> {
  const remote = await pickRemote(folderPath, 'Remove remote failed', 'Remove Remote', 'Pick a remote to remove:')
  if (!remote) return
  const name = remote.name
  const proceed = await confirmAsync({
    title: 'Remove Remote?',
    message: `Remove remote "${name}"? Its remote-tracking branches are deleted locally; the remote repository is untouched.`,
    confirmLabel: 'Remove Remote'
  })
  if (!proceed) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.removeRemote, () =>
    runGitAction(folderPath, (path) => backend.git.remoteConfig(path, { kind: 'remove', name }))
  )
}

export async function renameRemote(folderPath: string): Promise<void> {
  const remote = await pickRemote(folderPath, 'Rename remote failed', 'Rename Remote', 'Pick a remote to rename:')
  if (!remote) return
  const answer = await promptAsync({
    title: 'Rename Remote',
    value: remote.name,
    placeholder: 'Remote name',
    confirmLabel: 'Rename Remote'
  })
  const newName = answer?.trim()
  if (!newName || newName === remote.name) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.renameRemote, () =>
    runGitAction(folderPath, (path) =>
      backend.git.remoteConfig(path, { kind: 'rename', oldName: remote.name, newName })
    )
  )
}

export async function setRemoteUrl(folderPath: string): Promise<void> {
  const remote = await pickRemote(
    folderPath,
    'Set remote URL failed',
    'Set Remote URL',
    'Pick a remote to set its URL:'
  )
  if (!remote) return
  const answer = await promptAsync({
    title: 'Set Remote URL',
    value: remote.fetchUrl ?? '',
    placeholder: 'Repository URL',
    confirmLabel: 'Set Remote URL'
  })
  const url = answer?.trim()
  if (!url) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.setRemoteUrl, () =>
    runGitAction(folderPath, (path) => backend.git.remoteConfig(path, { kind: 'set_url', name: remote.name, url }))
  )
}

export async function deleteMergedBranches(folderPath: string): Promise<void> {
  let names: string[]
  try {
    names = await backend.git.mergedBranches(folderPath)
  } catch (error) {
    notify.error('Delete merged branches failed', getErrorMessage(error))
    return
  }
  if (names.length === 0) {
    notify.info('No merged branches', 'No local branches are fully merged into HEAD.')
    return
  }
  const n = names.length
  const proceed = await confirmAsync({
    title: 'Delete Merged Branches?',
    message: `Delete ${n} local branch${n === 1 ? '' : 'es'} merged into HEAD?\n\n${names.join('\n')}\n\nRemote branches are not touched.`,
    confirmLabel: 'Delete Branches'
  })
  if (!proceed) return
  await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.deleteMergedBranches, () =>
    runGitAction(folderPath, async (path) => {
      for (const name of names) await backend.git.branch(path, { kind: 'delete', name, force: false })
    })
  )
}

export function syncFolder(folderPath: string, rebase = false): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.sync, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'sync', rebase }))
  )
}

export type CommitVariant = { kind: 'commit'; post: PostCommit } | { kind: 'amend' } | { kind: 'all' }

/** Commit the folder's draft with its Sign Off / Skip Hooks flags; offers to stage everything when nothing is staged. */
export async function commitFolder(folderPath: string, variant: CommitVariant): Promise<void> {
  const summary = getGitSummary(folderPath)
  const state = getCommitState(folderPath)
  if (!summary) return
  // Capture the exact draft with its message before any prompt lets the user edit it.
  const submitted = state.draft
  const message = submitted.trim()
  if (variant.kind !== 'amend' && !message) return
  let all = variant.kind === 'all'
  if (variant.kind === 'commit' && summary.staged_count === 0) {
    if (summary.changes.length === 0) return
    const proceed = await confirmAsync({
      title: 'Stage All and Commit?',
      message:
        'There are no staged changes to commit. Would you like to stage all your changes and commit them directly?',
      confirmLabel: 'Stage All & Commit'
    })
    if (!proceed) return
    all = true
  }
  const hash = await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.commit, () =>
    runGitAction(folderPath, (path) =>
      backend.git.commit(path, message, {
        amend: variant.kind === 'amend',
        all,
        signOff: state.signOff,
        noVerify: state.skipHooks
      })
    )
  )
  if (!hash) return
  if (getCommitState(folderPath).draft === submitted) setCommitDraft(folderPath, '')
  setSkipHooks(folderPath, false)
  if (variant.kind === 'commit' && variant.post === 'push') await pushFolder(folderPath)
  else if (variant.kind === 'commit' && variant.post === 'sync') await syncFolder(folderPath)
}

export async function undoLastCommit(folderPath: string): Promise<string | undefined> {
  const proceed = await confirmAsync({
    title: 'Undo Last Commit?',
    message: 'This removes the last commit. Its changes stay staged and its message returns to the commit box.',
    confirmLabel: 'Undo Commit',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return undefined
  const message = await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.undoLastCommit, () =>
    runGitAction(folderPath, (path) => backend.git.undoLastCommit(path))
  )
  if (typeof message === 'string' && message.length > 0) setCommitDraft(folderPath, message)
  return message
}

export async function stageAllFolder(folderPath: string): Promise<boolean | undefined> {
  const conflicts = getGitSummary(folderPath)?.conflicts ?? []
  // VS Code parity: Stage All includes Merge Changes, but only through the same
  // deletion choices and marker confirmation as Mark Resolved.
  const plan = conflicts.length > 0 ? await planConflictResolution(conflicts) : null
  if (conflicts.length > 0 && !plan) return false
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.stageAll, async () =>
    plan && !(await applyConflictPlan(folderPath, plan)) ? false : stageAll(folderPath)
  )
}

export function unstageAllFolder(folderPath: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.unstageAll, () => unstageAll(folderPath))
}

export function discardAllFolder(folderPath: string): Promise<boolean | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.discardAll, () => discardAll(folderPath))
}

export function stashFolder(
  folderPath: string,
  scope: GitStashScope,
  message: string | null = null
): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.stash, () =>
    runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'push', message, scope }))
  )
}

/** VS Code stash flow: optional message prompt, then push; cancel aborts. */
export async function promptStash(folderPath: string, scope: GitStashScope): Promise<void> {
  const title =
    scope.kind === 'all'
      ? scope.includeUntracked
        ? 'Stash (Include Untracked)'
        : 'Stash'
      : scope.kind === 'staged'
        ? 'Stash Staged'
        : 'Stash Changes'
  const answer = await promptAsync({
    title,
    message: 'Optionally provide a stash message.',
    placeholder: 'Stash message',
    confirmLabel: 'Stash'
  })
  if (answer === null) return
  await stashFolder(folderPath, scope, answer.trim() || null)
}

export async function applyStash(folderPath: string, index: number): Promise<boolean> {
  const options = GIT_ACTION_NOTIFICATIONS.applyStash
  return (
    (await runNotifiedGitAction(
      folderPath,
      {
        ...options,
        loading: { ...options.loading, description: `stash@{${index}}` },
        success: { ...options.success, description: `stash@{${index}}` }
      },
      () => runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'apply', index })).then(() => true)
    )) === true
  )
}

export async function popStash(folderPath: string, index: number): Promise<boolean> {
  const options = GIT_ACTION_NOTIFICATIONS.applyStash
  return (
    (await runNotifiedGitAction(
      folderPath,
      {
        ...options,
        loading: { ...options.loading, description: `stash@{${index}}` },
        success: { ...options.success, description: `stash@{${index}}` }
      },
      () => runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'pop', index })).then(() => true)
    )) === true
  )
}

export async function popLatestStash(folderPath: string): Promise<void> {
  const count = await backend.git.stashCount(folderPath).catch((e) => {
    notify.error('Pop stash failed', getErrorMessage(e))
    return null
  })
  if (count === null) return
  if (count === 0) {
    notify.info('No stashes', 'There are no stashes in the repository.')
    return
  }
  await popStash(folderPath, 0)
}

export async function dropStash(folderPath: string, index: number): Promise<boolean> {
  const proceed = await confirmAsync({
    title: 'Drop Stash?',
    message: 'This will permanently delete this stash entry. This cannot be undone.',
    confirmLabel: 'Drop'
  })
  if (!proceed) return false
  const options = GIT_ACTION_NOTIFICATIONS.dropStash
  return (
    (await runNotifiedGitAction(
      folderPath,
      {
        ...options,
        loading: { ...options.loading, description: `stash@{${index}}` },
        success: { ...options.success, description: `stash@{${index}}` }
      },
      () => runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'drop', index })).then(() => true)
    )) === true
  )
}

export async function clearStashes(folderPath: string): Promise<boolean> {
  const proceed = await confirmAsync({
    title: 'Drop All Stashes?',
    message: 'This will permanently delete every stash entry. This cannot be undone.',
    confirmLabel: 'Drop All'
  })
  if (!proceed) return false
  return (
    (await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.clearStashes, () =>
      runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'clear' })).then(() => true)
    )) === true
  )
}

/** Copy text from a git menu; clipboard refusal (permissions, unfocused page) notifies instead of rejecting. */
export async function copyToClipboard(text: string, errorTitle = 'Copy failed'): Promise<void> {
  try {
    await platform.clipboard.writeText(text)
  } catch (e) {
    notify.error(errorTitle, getErrorMessage(e))
  }
}

export async function branchFromStash(folderPath: string, index: number): Promise<boolean> {
  const answer = await promptAsync({
    title: 'Create Branch from Stash',
    message: `Create and check out a branch from stash@{${index}}; the stash is dropped when it applies cleanly.`,
    placeholder: 'Branch name',
    confirmLabel: 'Create Branch'
  })
  const name = answer?.trim()
  if (!name) return false
  return (
    (await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.branchFromStash, () =>
      runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'branch', index, name })).then(() => true)
    )) === true
  )
}

export function ignoreFiles(folderPath: string, files: string[]): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.ignore, () =>
    runGitAction(folderPath, (path) => backend.git.paths(path, { kind: 'ignore', files }))
  )
}

export function fastForwardBranch(folderPath: string, name: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.fastForward, () =>
    runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'fast_forward', name }))
  )
}

export function rebaseOnto(folderPath: string, target: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.rebase, () =>
    runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'rebase', target }))
  )
}

export function unsetUpstream(folderPath: string, branch: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.unsetUpstream, () =>
    runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'unset_upstream', branch }))
  )
}

const pendingOperations = new SvelteSet<string>()

export function isOperationPending(folderPath: string): boolean {
  return pendingOperations.has(folderPath)
}

async function runSequencerAction(folderPath: string, action: GitSequencerAction): Promise<boolean> {
  const operation = getGitSummary(folderPath)?.operation
  if (!operation || pendingOperations.has(folderPath)) return false
  const display = describeOperation(operation)
  if (action.kind === 'skip' && !display.canSkip) return false
  const notification =
    action.kind === 'continue'
      ? GIT_ACTION_NOTIFICATIONS.continueOperation(display)
      : action.kind === 'abort'
        ? GIT_ACTION_NOTIFICATIONS.abortOperation(display)
        : GIT_ACTION_NOTIFICATIONS.skipOperation
  pendingOperations.add(folderPath)
  try {
    const succeeded =
      (await runNotifiedGitAction(folderPath, notification, async () => {
        // A failed continue/skip can have consumed a commit before the next conflict.
        // Await its refresh too, then rethrow through the normal notification path.
        const failure = await runGitAction(folderPath, (path) =>
          backend.git.sequencer(path, operation.kind, action).then(
            () => null,
            (error: unknown) => ({ error })
          )
        )
        if (failure) throw failure.error
        return true
      })) === true
    const current = getGitSummary(folderPath)?.operation
    return (
      succeeded ||
      (current !== undefined &&
        (current === null ||
          current.kind !== operation.kind ||
          current.target !== operation.target ||
          current.headName !== operation.headName ||
          current.commit?.hash !== operation.commit?.hash ||
          current.step !== operation.step))
    )
  } finally {
    pendingOperations.delete(folderPath)
  }
}

export async function continueOperation(folderPath: string): Promise<void> {
  // Rebase must keep Git's prepared message and original author together, so the
  // hidden draft is never submitted and therefore never consumed.
  if (getGitSummary(folderPath)?.operation?.kind === 'rebase') {
    await runSequencerAction(folderPath, { kind: 'continue', message: null })
    return
  }
  const state = getCommitState(folderPath)
  const submitted = state.draft
  const consumed = await runSequencerAction(folderPath, {
    kind: 'continue',
    message: getEditedCommitMessage(folderPath)
  })
  if (!consumed || getCommitState(folderPath) !== state || state.draft !== submitted) return
  setCommitDraft(folderPath, '')
  // The refresh may have advanced to another paused commit with the same message.
  const summary = getGitSummary(folderPath)
  if (summary) syncCommitTemplate(folderPath, summary)
}

export async function skipOperation(folderPath: string): Promise<void> {
  await runSequencerAction(folderPath, { kind: 'skip' })
}

export async function abortOperation(folderPath: string): Promise<void> {
  await runSequencerAction(folderPath, { kind: 'abort' })
}

export function takeConflictSide(
  folderPath: string,
  files: string[],
  side: GitConflictSide
): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.takeConflictSide, () =>
    runGitAction(folderPath, (path) => backend.git.conflicts(path, { kind: 'take_side', files, side }))
  )
}

interface ConflictPlan {
  staged: string[]
  deleted: string[]
}

/** Every question is answered before any mutation, so cancelling leaves the index untouched. */
async function planConflictResolution(conflicts: GitConflict[]): Promise<ConflictPlan | null> {
  const plan: ConflictPlan = { staged: [], deleted: [] }
  for (const conflict of conflicts) {
    if (conflict.kind === 'deleted_by_us' || conflict.kind === 'deleted_by_them') {
      const deletedByThem = conflict.kind === 'deleted_by_them'
      const choice = await pickAsync({
        title: 'Resolve Deletion Conflict',
        message: `File "${basename(conflict.path)}" was ${deletedByThem ? 'deleted by them and modified by us' : 'deleted by us and modified by them'}.\n\nWhat would you like to do?`,
        options: [
          { value: 'keep', label: deletedByThem ? 'Keep Our Version' : 'Keep Their Version' },
          { value: 'delete', label: 'Delete File' }
        ]
      })
      if (choice === null) return null
      if (choice === 'keep') plan.staged.push(conflict.path)
      else plan.deleted.push(conflict.path)
    } else if (conflict.kind === 'both_deleted') {
      plan.deleted.push(conflict.path)
    } else {
      plan.staged.push(conflict.path)
    }
  }
  return plan
}

/** Stage before deleting: only staging can still be cancelled (marker confirm). */
async function applyConflictPlan(folderPath: string, plan: ConflictPlan): Promise<boolean> {
  if (plan.staged.length > 0 && !(await stageFiles(folderPath, plan.staged))) return false
  if (plan.deleted.length > 0) {
    await runGitAction(folderPath, (path) => backend.git.conflicts(path, { kind: 'delete', files: plan.deleted }))
  }
  return true
}

export async function markResolved(folderPath: string, files: string[]): Promise<void> {
  const summary = getGitSummary(folderPath)
  if (!summary) return
  const paths = new Set(files)
  const conflicts = summary.conflicts.filter((conflict) => paths.has(conflict.path))
  if (conflicts.length === 0) return
  const plan = await planConflictResolution(conflicts)
  if (plan === null) return
  const resolved = await runNotifiedGitAction(
    folderPath,
    {
      loading: GIT_ACTION_NOTIFICATIONS.takeConflictSide.loading,
      error: GIT_ACTION_NOTIFICATIONS.takeConflictSide.error
    },
    () => applyConflictPlan(folderPath, plan)
  )
  if (resolved) notify.success('Conflicts resolved')
}

export async function pullActiveFolder(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (folderPath) await pullFolder(folderPath)
}

export async function pushActiveFolder(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (folderPath) await pushFolder(folderPath)
}

export async function fetchActiveFolder(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (folderPath) await fetchFolder(folderPath)
}

export async function forcePushActiveFolder(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  await forcePushWithLease(folderPath)
}

export async function undoLastCommitActiveFolder(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  await undoLastCommit(folderPath)
}

/** Add is never retried: a lost response may already have created this checkout. */
export async function createWorktree(
  folderPath: string,
  destination: string,
  checkout: GitWorktreeCheckout,
  onCreated?: () => void
): Promise<Extract<GitWorktreeResult, { kind: 'added' }>> {
  let result: GitWorktreeResult
  try {
    result = await runGitAction(folderPath, (path) =>
      backend.git.worktree(path, { kind: 'add', destination, checkout })
    )
  } catch (error) {
    await refreshWorktrees(folderPath)
    throw error
  }
  if (result.kind !== 'added') throw new Error('The host returned an unexpected worktree creation result.')
  onCreated?.()
  try {
    const inventory = await refreshWorktrees(folderPath)
    if (inventory) await refreshWorktreeRepository(inventory)
    if (result.warnings.length > 0 || !result.copyComplete) {
      notify.warning(
        result.copyComplete ? 'Worktree setup warnings' : 'Worktree setup incomplete',
        `${result.path}\n${result.warnings.join('\n')}${!result.copyComplete ? '\nAutomatic setup tasks were skipped.' : ''}`
      )
    }
    await openFolder(result.path)
    if (!getTabs().some((tab) => tab.folderPath === result.path)) {
      notify.error(
        'Worktree created but not opened',
        `Open ${result.path} from Worktrees. Git creation will not be retried.`
      )
      return result
    }
    notify.success('Worktree created', result.path)
    if (!result.copyComplete) return result
    try {
      const tasks = await backend.tasks.list(result.path)
      for (const task of tasks) {
        if (task.runOn !== 'worktreeCreated') continue
        try {
          const tabId = await openTaskTab(result.path, task)
          if (tabId !== null) await startTaskTab(tabId, task.clearOnRerun ?? false)
        } catch (error) {
          notify.error(`Setup task failed: ${task.label}`, getErrorMessage(error))
        }
      }
    } catch (error) {
      notify.error(
        'Worktree task discovery failed',
        `${result.path}\n${getErrorMessage(error)} The checkout was retained.`
      )
    }
  } catch (error) {
    notify.error(
      'Worktree created; setup failed',
      `${result.path}\n${getErrorMessage(error)} Git creation will not be retried.`
    )
  }
  return result
}

export async function removeWorktree(folderPath: string, worktreePath: string): Promise<void> {
  await runNotifiedTask(
    async () => {
      const inventory = await backend.git.listWorktrees(folderPath)
      const member = inventory.worktrees.find((worktree) => worktree.path === worktreePath)
      if (!member || member.isMain || member.locked !== null || member.prunable !== null || member.bare) {
        throw new Error('Only a live, unlocked linked worktree can be removed. Use Prune for stale registrations.')
      }
      const wasActive = isPathWithin(worktreePath, getActiveFolderPath() ?? '')
      const proceed = await confirmAsync({
        title: 'Remove Worktree?',
        message: `${worktreePath}\n\nAssociated tabs, sessions and tasks will close. The branch is retained. Sworm stops its managed processes only; stop external terminals and independently daemonized commands before continuing.`,
        confirmLabel: 'Remove Worktree'
      })
      if (!proceed || !(await closeWorktreeTabs([worktreePath]))) return
      try {
        await runGitAction(folderPath, (path) =>
          backend.git.worktree(path, { kind: 'remove', worktreePath, force: false })
        )
      } catch (error) {
        if (!isGitError(error, 'worktreeHasChanges')) throw error
        const force = await confirmAsync({
          title: 'Force Delete Worktree?',
          message: `${worktreePath}\n\nTracked, untracked and ignored files in this worktree will be permanently deleted. The branch is retained. This cannot be undone.`,
          confirmLabel: 'Force Delete'
        })
        if (!force) return
        if (!(await closeWorktreeTabs([worktreePath]))) return
        await runGitAction(folderPath, (path) =>
          backend.git.worktree(path, { kind: 'remove', worktreePath, force: true })
        )
      }
      forgetClosedWorktreeTabs(worktreePath)
      forgetWorktree(worktreePath)
      try {
        await forgetRecentWorktree(worktreePath)
      } catch (error) {
        notify.warning('Worktree removed; recents cleanup failed', getErrorMessage(error))
      }
      await refreshWorktreeRepository(inventory)
      if (wasActive && inventory.mainPath) {
        await openFolder(inventory.mainPath)
        if (!getTabs().some((tab) => tab.folderPath === inventory.mainPath)) {
          notify.error(
            'Open primary checkout failed',
            `The worktree was removed. The primary checkout remains at ${inventory.mainPath}.`
          )
        }
      }
      notify.success('Worktree removed', worktreePath)
    },
    { loading: { title: 'Removing worktree', description: worktreePath }, error: { title: 'Remove worktree failed' } }
  )
}

export async function pruneWorktrees(folderPath: string): Promise<void> {
  await runNotifiedTask(
    async () => {
      const inventory = await backend.git.listWorktrees(folderPath)
      await runGitAction(folderPath, (path) => backend.git.worktree(path, { kind: 'prune' }))
      await refreshWorktreeRepository(inventory)
    },
    {
      loading: { title: 'Pruning worktrees' },
      success: { title: 'Stale worktree registrations pruned' },
      error: { title: 'Prune worktrees failed' }
    }
  )
}

export async function migrateWorktreeChanges(folderPath: string, worktreePath: string): Promise<void> {
  await runNotifiedTask(
    async () => {
      const inventory = await backend.git.listWorktrees(folderPath)
      const source = inventory.worktrees.find((worktree) => worktree.path === worktreePath)
      const mainPath = inventory.mainPath
      if (!source || source.isMain || source.bare || source.prunable !== null || !mainPath) {
        throw new Error('Migration requires a live linked worktree and a primary checkout.')
      }
      const proceed = await confirmAsync({
        title: 'Migrate Changes to Main?',
        message: `Source: ${worktreePath}\nMain: ${mainPath}\n\nCaller-owned tabs, sessions and tasks in both folders will close. Main must be clean. Staged, unstaged and nonignored untracked changes move to main; ignored files stay behind. The source worktree stays and a named recovery stash is retained, including on success.\n\nStop external terminals and independently daemonized commands first. Sworm can stop only its managed processes.`,
        confirmLabel: 'Migrate Changes'
      })
      if (!proceed || !(await closeWorktreeTabs([worktreePath, mainPath]))) return
      try {
        const result = await runGitAction(folderPath, (path) =>
          backend.git.worktree(path, { kind: 'migrate', worktreePath })
        )
        if (result.kind !== 'migrated') throw new Error('The host returned an unexpected migration result.')
        notify.success(
          'Changes migrated to main',
          result.recoveryStash
            ? `Recovery stash retained: ${result.recoveryStash}`
            : 'The source worktree had no changes to migrate.'
        )
      } finally {
        await openFolder(mainPath)
        if (getTabs().some((tab) => tab.folderPath === mainPath)) revealGitChanges()
        else notify.error('Open primary checkout failed', `Open ${mainPath} to inspect migration state.`)
        await refreshWorktreeRepository(inventory)
      }
    },
    { loading: { title: 'Migrating worktree changes' }, error: { title: 'Worktree migration failed' } }
  )
}
