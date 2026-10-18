import type { GitSummary } from '$lib/types/backend'
import { describeOperation } from './operation'

export type PostCommit = 'none' | 'push' | 'sync'
export type PrimaryAction =
  | { kind: 'commit'; post: PostCommit; enabled: boolean }
  | { kind: 'continue'; title: string; enabled: boolean }
  | { kind: 'publish'; branch: string }
  | { kind: 'sync'; ahead: number; behind: number }

/** Paused sequences take priority; otherwise commit → publish → sync → disabled commit. */
export function resolvePrimaryAction(summary: GitSummary, message: string, postCommit: PostCommit): PrimaryAction {
  if (summary.operation !== null) {
    const display = describeOperation(summary.operation)
    return { kind: 'continue', title: `Continue ${display.title}`, enabled: summary.conflicts.length === 0 }
  }
  const onBranch = summary.branch !== null && !summary.detached
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
