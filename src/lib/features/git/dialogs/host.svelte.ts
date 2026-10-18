import * as branches from '$lib/features/git/branches.svelte'
import type { BranchSummary, GitWorktreeCheckout } from '$lib/types/backend'

export type GitDialogRequest =
  | { kind: 'create_branch'; folderPath: string; base: string }
  | { kind: 'create_worktree'; folderPath: string; checkout?: GitWorktreeCheckout }
  | { kind: 'rename_branch'; folderPath: string; branch: string }
  | { kind: 'delete_branch'; folderPath: string; branch: BranchSummary }
  | { kind: 'set_upstream'; folderPath: string; branch: string; initialUpstream: string }
  | { kind: 'merge'; folderPath: string; source: string; current: string }

let current = $state<{ id: number; request: GitDialogRequest } | null>(null)
let nextId = 1

export function getGitDialog(): { id: number; request: GitDialogRequest } | null {
  return current
}

/** One git dialog at a time; a new request replaces an open one. Branch-backed dialogs load the branch list first. */
export function openGitDialog(request: GitDialogRequest): void {
  if (request.kind === 'create_branch' || request.kind === 'create_worktree' || request.kind === 'set_upstream') {
    void branches.loadFor(request.folderPath, { autoFetch: false })
  }
  current = { id: nextId++, request }
}

export function closeGitDialog(id: number): void {
  if (current?.id === id) current = null
}
