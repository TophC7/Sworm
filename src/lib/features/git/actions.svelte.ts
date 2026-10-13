import { backend } from '$lib/api/backend'
import { revealGitChanges } from '$lib/features/app-shell/sidebar/state.svelte'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { isGitError } from '$lib/features/git/errors'
import { discardAll, getGitSummary, runGitAction, stageAll, unstageAll } from '$lib/features/git/state.svelte'
import { runNotifiedTask, type RunNotifiedTaskOptions } from '$lib/features/notifications/runNotifiedTask'
import { notify } from '$lib/features/notifications/state.svelte'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'
import type { GitCommitOptions, GitConflictSide, GitStashScope } from '$lib/types/backend'

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
  continueOperation: (noun: string) => ({
    loading: { title: `Continuing ${noun}` },
    success: { title: `${noun[0].toUpperCase()}${noun.slice(1)} continued` },
    error: { title: 'Continue failed' }
  }),
  skipOperation: {
    loading: { title: 'Skipping commit' },
    success: { title: 'Commit skipped' },
    error: { title: 'Skip failed' }
  },
  abortOperation: (noun: string) => ({
    loading: { title: `Aborting ${noun}` },
    success: { title: `${noun[0].toUpperCase()}${noun.slice(1)} aborted` },
    error: { title: 'Abort failed' }
  }),
  takeConflictSide: {
    loading: { title: 'Resolving conflicts' },
    success: { title: 'Conflicts resolved' },
    error: { title: 'Resolve failed' }
  },
  deleteConflicted: {
    loading: { title: 'Deleting files' },
    success: { title: 'Files deleted' },
    error: { title: 'Delete failed' }
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
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'pull', rebase })))
}

export function pushFolder(folderPath: string, branch: string | null = null): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.push, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'push', branch, force: false })))
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
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'push', branch, force: true })))
}

export function fetchFolder(folderPath: string, remote: string | null = null): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.fetch, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'fetch', remote })))
}

export function publishBranch(
  folderPath: string,
  branch: string | null = null,
  remote: string | null = null
): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.publish, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'publish', branch, remote })))
}

export function syncFolder(folderPath: string, rebase = false): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.sync, () =>
    runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'sync', rebase })))
}

export async function commitFolder(
  folderPath: string,
  message: string,
  options: Partial<GitCommitOptions> = {},
  after: 'push' | 'sync' | null = null
): Promise<string | undefined> {
  const hash = await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.commit, () =>
    runGitAction(folderPath, (path) => backend.git.commit(path, message, options)))
  if (hash && after === 'push') await pushFolder(folderPath)
  if (hash && after === 'sync') await syncFolder(folderPath)
  return hash
}

export async function undoLastCommit(folderPath: string): Promise<string | undefined> {
  const proceed = await confirmAsync({
    title: 'Undo Last Commit?',
    message: 'This will soft-reset to HEAD~1. Your changes will be preserved as staged files.',
    confirmLabel: 'Undo Commit',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return undefined
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.undoLastCommit, () =>
    runGitAction(folderPath, (path) => backend.git.undoLastCommit(path)))
}

export function stageAllFolder(folderPath: string): Promise<boolean | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.stageAll, () => stageAll(folderPath))
}

export function unstageAllFolder(folderPath: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.unstageAll, () => unstageAll(folderPath))
}

export function discardAllFolder(folderPath: string): Promise<boolean | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.discardAll, () => discardAll(folderPath))
}

export function stashFolder(folderPath: string, scope: GitStashScope, message: string | null = null): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.stash, () =>
    runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'push', message, scope })))
}

export async function applyStash(folderPath: string, index: number): Promise<boolean> {
  const options = GIT_ACTION_NOTIFICATIONS.applyStash
  return (await runNotifiedGitAction(folderPath, {
    ...options,
    loading: { ...options.loading, description: `stash@{${index}}` },
    success: { ...options.success, description: `stash@{${index}}` }
  }, () => runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'apply', index })).then(() => true))) === true
}

export async function popStash(folderPath: string, index: number): Promise<boolean> {
  const options = GIT_ACTION_NOTIFICATIONS.applyStash
  return (await runNotifiedGitAction(folderPath, {
    ...options,
    loading: { ...options.loading, description: `stash@{${index}}` },
    success: { ...options.success, description: `stash@{${index}}` }
  }, () => runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'pop', index })).then(() => true))) === true
}

export async function dropStash(folderPath: string, index: number): Promise<boolean> {
  const proceed = await confirmAsync({
    title: 'Drop Stash?',
    message: 'This will permanently delete this stash entry. This cannot be undone.',
    confirmLabel: 'Drop'
  })
  if (!proceed) return false
  const options = GIT_ACTION_NOTIFICATIONS.dropStash
  return (await runNotifiedGitAction(folderPath, {
    ...options,
    loading: { ...options.loading, description: `stash@{${index}}` },
    success: { ...options.success, description: `stash@{${index}}` }
  }, () => runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'drop', index })).then(() => true))) === true
}

export async function clearStashes(folderPath: string): Promise<boolean> {
  const proceed = await confirmAsync({
    title: 'Drop All Stashes?',
    message: 'This will permanently delete every stash entry. This cannot be undone.',
    confirmLabel: 'Drop All'
  })
  if (!proceed) return false
  return (await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.clearStashes, () =>
    runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'clear' })).then(() => true))) === true
}

export async function branchFromStash(folderPath: string, index: number, name: string): Promise<boolean> {
  return (await runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.branchFromStash, () =>
    runGitAction(folderPath, (path) => backend.git.stash(path, { kind: 'branch', index, name })).then(() => true))) === true
}

export function ignoreFiles(folderPath: string, files: string[]): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.ignore, () =>
    runGitAction(folderPath, (path) => backend.git.paths(path, { kind: 'ignore', files })))
}

export function fastForwardBranch(folderPath: string, name: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.fastForward, () =>
    runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'fast_forward', name })))
}

export function rebaseOnto(folderPath: string, target: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.rebase, () =>
    runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'rebase', target })))
}

export function unsetUpstream(folderPath: string, branch: string): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.unsetUpstream, () =>
    runGitAction(folderPath, (path) => backend.git.branch(path, { kind: 'unset_upstream', branch })))
}

export function continueOperation(folderPath: string, message: string | null = null): Promise<void | undefined> | undefined {
  const kind = getGitSummary(folderPath)?.operation?.kind
  if (!kind) return
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.continueOperation(kind.replace('_', '-')), () =>
    runGitAction(folderPath, (path) => backend.git.sequencer(path, kind, { kind: 'continue', message })))
}

export function skipOperation(folderPath: string): Promise<void | undefined> | undefined {
  const kind = getGitSummary(folderPath)?.operation?.kind
  if (!kind) return
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.skipOperation, () =>
    runGitAction(folderPath, (path) => backend.git.sequencer(path, kind, { kind: 'skip' })))
}

export function abortOperation(folderPath: string): Promise<void | undefined> | undefined {
  const kind = getGitSummary(folderPath)?.operation?.kind
  if (!kind) return
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.abortOperation(kind.replace('_', '-')), () =>
    runGitAction(folderPath, (path) => backend.git.sequencer(path, kind, { kind: 'abort' })))
}

export function takeConflictSide(folderPath: string, files: string[], side: GitConflictSide): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.takeConflictSide, () =>
    runGitAction(folderPath, (path) => backend.git.conflicts(path, { kind: 'take_side', files, side })))
}

export function deleteConflicted(folderPath: string, files: string[]): Promise<void | undefined> {
  return runNotifiedGitAction(folderPath, GIT_ACTION_NOTIFICATIONS.deleteConflicted, () =>
    runGitAction(folderPath, (path) => backend.git.conflicts(path, { kind: 'delete', files })))
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
