// Folder-keyed git state module using Svelte 5 runes.
//
// Provides folder-keyed git summaries and managed polling so that
// multiple components (sidebar, status bar) can read git state without
// duplicating backend calls.

import { backend } from '$lib/api/backend'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import type { GitSummary } from '$lib/types/backend'

const GIT_POLL_INTERVAL_MS = 10_000

const gitStore = createFolderKeyedStore<GitSummary>()

function summariesEqual(a: GitSummary | null | undefined, b: GitSummary): boolean {
  if (!a) return false
  if (
    a.is_repo !== b.is_repo ||
    a.branch !== b.branch ||
    a.base_ref !== b.base_ref ||
    a.ahead !== b.ahead ||
    a.behind !== b.behind ||
    a.staged_count !== b.staged_count ||
    a.unstaged_count !== b.unstaged_count ||
    a.untracked_count !== b.untracked_count ||
    a.changes.length !== b.changes.length
  ) {
    return false
  }

  for (let i = 0; i < a.changes.length; i++) {
    const left = a.changes[i]
    const right = b.changes[i]
    if (
      left.path !== right.path ||
      left.status !== right.status ||
      left.staged !== right.staged ||
      left.additions !== right.additions ||
      left.deletions !== right.deletions
    ) {
      return false
    }
  }

  return true
}

// READ //
export function getGitSummary(folderPath: string): GitSummary | null {
  return gitStore.get(folderPath) ?? null
}

// WRITE //
export async function refreshGit(folderPath: string): Promise<void> {
  try {
    const summary = await backend.git.getSummary(folderPath)
    if (summariesEqual(gitStore.get(folderPath), summary)) {
      return
    }
    gitStore.set(folderPath, summary)
  } catch (e) {
    console.error(`Failed to refresh git for ${folderPath}:`, e)
  }
}

export function startGitPolling(folderPath: string) {
  void refreshGit(folderPath)
  gitStore.startPolling(folderPath, {
    intervalMs: GIT_POLL_INTERVAL_MS,
    tick: refreshGit
  })
}

export function stopGitPolling(folderPath: string) {
  gitStore.stopPolling(folderPath)
}

/** Forget the folder's summary and stop its polling; called when the workbench releases the folder. */
export function releaseGitFolder(folderPath: string) {
  gitStore.delete(folderPath)
}

/**
 * Run a git operation against a folder, then refresh git state.
 * Errors propagate to the caller.
 */
export async function runGitAction<T>(folderPath: string, fn: (path: string) => Promise<T>): Promise<T> {
  const result = await fn(folderPath)
  await refreshGit(folderPath)
  return result
}
