import { backend } from '$lib/api/backend'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { getActiveProject } from '$lib/features/projects/state.svelte'
import { getGitActionNotifications, type GitActionKind } from '$lib/features/git/actionNotifications'
import { runGitAction } from '$lib/features/git/state.svelte'
import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'

async function runForActiveProject(kind: GitActionKind, fn: (path: string) => Promise<unknown>): Promise<unknown> {
  const project = getActiveProject()
  if (!project) return undefined
  return runGitActionForProject(project.id, project.path, kind, fn)
}

export async function runGitActionForProject(
  projectId: string,
  projectPath: string,
  kind: GitActionKind,
  fn: (path: string) => Promise<unknown>
): Promise<unknown> {
  return runNotifiedTask(() => runGitAction(projectId, projectPath, fn), getGitActionNotifications(kind))
}

export async function pullActiveProject(): Promise<void> {
  await runForActiveProject('pull', (path) => backend.git.pull(path))
}

export async function pushActiveProject(): Promise<void> {
  await runForActiveProject('push', (path) => backend.git.push(path))
}

export async function fetchActiveProject(): Promise<void> {
  await runForActiveProject('fetch', (path) => backend.git.fetch(path))
}

export async function forcePushActiveProject(): Promise<void> {
  const project = getActiveProject()
  if (!project) return
  await forcePushWithLease(project.id, project.path)
}

export async function forcePushWithLease(projectId: string, projectPath: string): Promise<void> {
  const proceed = await confirmAsync({
    title: 'Force Push?',
    message:
      'This will push with --force-with-lease. Remote commits may be overwritten if your local branch is ahead of the remote.',
    confirmLabel: 'Force Push',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return
  await runGitActionForProject(projectId, projectPath, 'forcePush', (path) => backend.git.pushForceWithLease(path))
}

export async function undoLastCommitActiveProject(): Promise<void> {
  const project = getActiveProject()
  if (!project) return
  await undoLastCommit(project.id, project.path)
}

export async function undoLastCommit(projectId: string, projectPath: string): Promise<string | undefined> {
  const proceed = await confirmAsync({
    title: 'Undo Last Commit?',
    message: 'This will soft-reset to HEAD~1. Your changes will be preserved as staged files.',
    confirmLabel: 'Undo Commit',
    cancelLabel: 'Cancel'
  })
  if (!proceed) return undefined
  const result = await runGitActionForProject(projectId, projectPath, 'undoLastCommit', (path) =>
    backend.git.undoLastCommit(path)
  )
  return typeof result === 'string' ? result : undefined
}
