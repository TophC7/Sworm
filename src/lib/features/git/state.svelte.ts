// Per-project git state module using Svelte 5 runes.
//
// Extracted from ProjectMainView's inline $effect + setInterval.
// Provides project-keyed git summaries and managed polling so that
// multiple components (sidebar, status bar) can read git state without
// duplicating backend calls.

import { backend } from '$lib/api/backend'
import type { GitSummary } from '$lib/types/backend'

const GIT_POLL_INTERVAL_MS = 10_000

let gitSummaries = $state<Map<string, GitSummary>>(new Map())

// Polling intervals keyed by projectId; not reactive, internal bookkeeping
const pollingIntervals = new Map<string, ReturnType<typeof setInterval>>()
// Project paths needed for polling (since we poll by projectId but fetch by path)
const projectPaths = new Map<string, string>()
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
  return gitSummaries.get(projectId) ?? null
}

// WRITE //
export async function refreshGit(projectId: string, projectPath?: string): Promise<void> {
  const path = projectPath ?? projectPaths.get(projectId)
  if (!path) return

  // Store the path for future refreshes
  projectPaths.set(projectId, path)

  try {
    const summary = await backend.git.getSummary(path)
    if (summariesEqual(gitSummaries.get(projectId), summary)) {
      return
    }
    gitSummaries = new Map(gitSummaries).set(projectId, summary)
  } catch (e) {
    console.error(`Failed to refresh git for ${projectId}:`, e)
  }
}

export function startGitPolling(projectId: string, projectPath: string) {
  // Store path and do an initial fetch
  projectPaths.set(projectId, projectPath)
  void refreshGit(projectId, projectPath)

  // Don't duplicate intervals
  if (pollingIntervals.has(projectId)) return

  const interval = setInterval(() => {
    void refreshGit(projectId)
  }, GIT_POLL_INTERVAL_MS)

  pollingIntervals.set(projectId, interval)
}

export function stopGitPolling(projectId: string) {
  const interval = pollingIntervals.get(projectId)
  if (interval) {
    clearInterval(interval)
    pollingIntervals.delete(projectId)
  }
  projectPaths.delete(projectId)
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
  stopGitPolling(projectId)
  const next = new Map(gitSummaries)
  next.delete(projectId)
  gitSummaries = next
}
