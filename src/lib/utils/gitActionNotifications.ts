import type { RunNotifiedTaskOptions } from '$lib/utils/notifiedTask'

export type GitActionKind =
  | 'pull'
  | 'push'
  | 'fetch'
  | 'forcePush'
  | 'undoLastCommit'
  | 'stageAll'
  | 'unstageAll'
  | 'discardAll'
  | 'stashAll'

const GIT_ACTION_NOTIFICATIONS: Record<GitActionKind, RunNotifiedTaskOptions<void>> = {
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
    success: { title: 'All changes discarded' },
    error: { title: 'Discard all failed' }
  },
  stashAll: {
    loading: { title: 'Stashing changes' },
    success: { title: 'Changes stashed' },
    error: { title: 'Stash failed' }
  }
}

export const gitCommitNotifications: RunNotifiedTaskOptions<string> = {
  loading: { title: 'Creating commit' },
  success: {
    title: 'Commit created',
    description: (hash) => hash.slice(0, 7)
  },
  error: { title: 'Commit failed' }
}

export function getGitActionNotifications(kind: GitActionKind): RunNotifiedTaskOptions<void> {
  return GIT_ACTION_NOTIFICATIONS[kind]
}
