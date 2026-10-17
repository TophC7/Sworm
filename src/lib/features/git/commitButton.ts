import type { GitSummary } from '$lib/types/backend'

export type PostCommit = 'none' | 'push' | 'sync'
export type PrimaryAction =
  | { kind: 'commit'; post: PostCommit; enabled: boolean }
  | { kind: 'publish'; branch: string }
  | { kind: 'sync'; ahead: number; behind: number }

/** VS Code action-button order: commit (changes) → publish (no upstream) → sync (ahead/behind) → disabled commit. */
export function resolvePrimaryAction(summary: GitSummary, message: string, postCommit: PostCommit): PrimaryAction {
  const onBranch = summary.branch !== null && !summary.detached && summary.operation === null
  const commit: PrimaryAction = {
    kind: 'commit',
    post: onBranch ? postCommit : 'none',
    enabled: summary.changes.length > 0 && message.trim().length > 0
  }
  if (summary.changes.length > 0) return commit
  if (onBranch && !summary.unborn) {
    if (!summary.upstream) return { kind: 'publish', branch: summary.branch! }
    const ahead = summary.ahead ?? 0
    const behind = summary.behind ?? 0
    if (ahead > 0 || behind > 0) return { kind: 'sync', ahead, behind }
  }
  return { ...commit, enabled: false }
}
