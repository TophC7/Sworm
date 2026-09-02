import { backend } from '$lib/api/backend'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { getGitActionNotifications, type GitActionKind } from '$lib/features/git/actionNotifications'
import { runGitAction } from '$lib/features/git/state.svelte'
import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'

function runNotifiedGitAction(
  folderPath: string,
  kind: GitActionKind,
  fn: (path: string) => Promise<unknown>
): Promise<unknown> {
  return runNotifiedTask(() => runGitAction(folderPath, fn), getGitActionNotifications(kind))
}

async function runForActiveFolder(kind: GitActionKind, fn: (path: string) => Promise<unknown>): Promise<unknown> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return undefined
  return runNotifiedGitAction(folderPath, kind, fn)
}

export async function pullActiveFolder(): Promise<void> {
  await runForActiveFolder('pull', (path) => backend.git.pull(path))
}

export async function pushActiveFolder(): Promise<void> {
  await runForActiveFolder('push', (path) => backend.git.push(path))
}

export async function fetchActiveFolder(): Promise<void> {
  await runForActiveFolder('fetch', (path) => backend.git.fetch(path))
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
  await runNotifiedGitAction(folderPath, 'forcePush', (path) => backend.git.pushForceWithLease(path))
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
  const result = await runNotifiedGitAction(folderPath, 'undoLastCommit', (path) => backend.git.undoLastCommit(path))
  return typeof result === 'string' ? result : undefined
}
