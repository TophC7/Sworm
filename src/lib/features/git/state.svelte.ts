// Per-project git state module using Svelte 5 runes.
//
// Extracted from ProjectMainView's inline $effect + setInterval.
// Provides project-keyed git summaries and managed polling so that
// multiple components (sidebar, status bar) can read git state without
// duplicating backend calls.

import { backend } from '$lib/api/backend'
import { createProjectKeyedStore } from '$lib/state/projectKeyedStore.svelte'
import type { GitSummary } from '$lib/types/backend'

const GIT_POLL_INTERVAL_MS = 10_000

const gitStore = createProjectKeyedStore<GitSummary>()

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
export function getGitSummary(projectId: string): GitSummary | null {
  return gitStore.get(projectId) ?? null
}

// WRITE //
export async function refreshGit(projectId: string, projectPath?: string): Promise<void> {
  const path = gitStore.resolveProjectPath(projectId, projectPath)
  if (!path) return

  try {
    const summary = await backend.git.getSummary(path)
    if (summariesEqual(gitStore.get(projectId), summary)) {
      return
    }
    gitStore.set(projectId, summary)
  } catch (e) {
    console.error(`Failed to refresh git for ${projectId}:`, e)
  }
}

export function startGitPolling(projectId: string, projectPath: string) {
  void refreshGit(projectId, projectPath)
  gitStore.startPolling(projectId, projectPath, {
    intervalMs: GIT_POLL_INTERVAL_MS,
    tick: refreshGit
  })
}

export function stopGitPolling(projectId: string) {
  gitStore.stopPolling(projectId)
}

/**
 * Run a git operation against a project, then refresh git state.
 * Errors propagate to the caller.
 */
export async function runGitAction<T>(
  projectId: string,
  projectPath: string,
  fn: (path: string) => Promise<T>
): Promise<T> {
  const result = await fn(projectPath)
  await refreshGit(projectId, projectPath)
  return result
}

export function clearGitState(projectId: string) {
  gitStore.clearFor(projectId)
}
