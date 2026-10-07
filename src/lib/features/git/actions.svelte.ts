import { backend } from '$lib/api/backend'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { discardAll, runGitAction, stageAll, unstageAll } from '$lib/features/git/state.svelte'
import { runNotifiedTask, type RunNotifiedTaskOptions } from '$lib/features/notifications/runNotifiedTask'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'

type GitActionKind =
  | 'pull'
  | 'push'
  | 'fetch'
  | 'forcePush'
  | 'undoLastCommit'
  | 'stageAll'
  | 'unstageAll'
  | 'discardAll'
  | 'stashAll'

const GIT_ACTION_NOTIFICATIONS: Record<GitActionKind, RunNotifiedTaskOptions<unknown>> = {
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
  fetch: {
    loading: { title: 'Fetching remote changes' },
    success: { title: 'Fetch complete' },
    error: { title: 'Fetch failed' }
  },
  forcePush: {
    loading: { title: 'Force pushing changes' },
    success: { title: 'Force push complete' },
    error: { title: 'Force push failed' }
  },
  undoLastCommit: {
    loading: { title: 'Undoing last commit' },
    success: { title: 'Commit undone' },
    error: { title: 'Undo commit failed' }
  },
  stageAll: {
    loading: { title: 'Staging all changes' },
    success: { title: 'All changes staged' },
    error: { title: 'Stage all failed' }
  },
  unstageAll: {
    loading: { title: 'Unstaging all changes' },
    success: { title: 'All changes unstaged' },
    error: { title: 'Unstage all failed' }
  },
  discardAll: {
    loading: { title: 'Discarding all changes' },
    success: { title: (ran) => (ran ? 'All changes discarded' : 'Discard cancelled') },
    error: { title: 'Discard all failed' }
  },
  stashAll: {
    loading: { title: 'Stashing changes' },
    success: { title: 'Changes stashed' },
    error: { title: 'Stash failed' }
  }
}

const gitCommitNotifications: RunNotifiedTaskOptions<string> = {
  loading: { title: 'Creating commit' },
  success: {
    title: 'Commit created',
    description: (hash) => hash.slice(0, 7)
  },
  error: { title: 'Commit failed' }
}

function runNotifiedGitAction(kind: GitActionKind, task: () => Promise<unknown>): Promise<unknown> {
  return runNotifiedTask(task, GIT_ACTION_NOTIFICATIONS[kind])
}

export function pullFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('pull', () => runGitAction(folderPath, (path) => backend.git.pull(path)))
}

export function pushFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('push', () => runGitAction(folderPath, (path) => backend.git.push(path)))
}

export function fetchFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('fetch', () => runGitAction(folderPath, (path) => backend.git.fetch(path)))
}

export function stashAllFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('stashAll', () => runGitAction(folderPath, (path) => backend.git.stashAll(path)))
}

export function stageAllFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('stageAll', () => stageAll(folderPath))
}

export function unstageAllFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('unstageAll', () => unstageAll(folderPath))
}

export function discardAllFolder(folderPath: string): Promise<unknown> {
  return runNotifiedGitAction('discardAll', () => discardAll(folderPath))
}

export function commitFolder(folderPath: string, message: string): Promise<string | undefined> {
  return runNotifiedTask(() => runGitAction(folderPath, (path) => backend.git.commit(path, message)), gitCommitNotifications)
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

export async function forcePushWithLease(folderPath: string): Promise<void> {
  const proceed = await confirmAsync({
    title: 'Force Push?',
    message:
      'This will push with --force-with-lease. Remote commits may be overwritten if your local branch is ahead of the remote.',
    confirmLabel: 'Force Push',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return
  await runNotifiedGitAction('forcePush', () => runGitAction(folderPath, (path) => backend.git.pushForceWithLease(path)))
}

export async function undoLastCommitActiveFolder(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  await undoLastCommit(folderPath)
}

export async function undoLastCommit(folderPath: string): Promise<string | undefined> {
  const proceed = await confirmAsync({
    title: 'Undo Last Commit?',
    message: 'This will soft-reset to HEAD~1. Your changes will be preserved as staged files.',
    confirmLabel: 'Undo Commit',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return undefined
  const result = await runNotifiedGitAction('undoLastCommit', () => runGitAction(folderPath, (path) => backend.git.undoLastCommit(path)))
  return typeof result === 'string' ? result : undefined
}
