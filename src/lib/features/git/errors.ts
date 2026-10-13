import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { basename } from '$lib/utils/paths'

export type GitError =
  | { kind: 'noUpstream'; branch: string; message: string }
  | { kind: 'pushRejected'; message: string }
  | { kind: 'mergeConflicts'; message: string }
  | { kind: 'branchInWorktree'; branch: string; path: string; message: string }
  | { kind: 'worktreeHasChanges'; path: string; message: string }
  | { kind: 'conflictMarkers'; paths: string[]; message: string }
  | { kind: 'branchUnmerged'; branch: string; message: string }
  | { kind: 'dirtyWorktree'; message: string }

export function isGitError<K extends GitError['kind']>(error: unknown, kind: K): error is Extract<GitError, { kind: K }> {
  return typeof error === 'object' && error !== null && 'kind' in error && error.kind === kind
}

/** Confirm staging files with conflict markers. Never call inside confirmAsync's run callback. */
export async function withConflictMarkerConfirm(stage: (allow: boolean) => Promise<void>): Promise<boolean> {
  try {
    await stage(false)
    return true
  } catch (error) {
    if (!isGitError(error, 'conflictMarkers')) throw error
    const message = error.paths.length === 1
      ? `Are you sure you want to stage ${basename(error.paths[0])} with merge conflicts?`
      : `Are you sure you want to stage ${error.paths.length} files with merge conflicts?`
    const proceed = await confirmAsync({
      title: 'Stage With Conflicts?',
      message,
      confirmLabel: 'Stage'
    })
    if (proceed) await stage(true)
    return proceed
  }
}
