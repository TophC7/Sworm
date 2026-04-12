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

// Polling intervals keyed by projectId — not reactive, internal bookkeeping
const pollingIntervals = new Map<string, ReturnType<typeof setInterval>>()
// Project paths needed for polling (since we poll by projectId but fetch by path)
const projectPaths = new Map<string, string>()

// ---------------------------------------------------------------------------
// Read
// ---------------------------------------------------------------------------

export function getGitSummary(projectId: string): GitSummary | null {
  return gitSummaries.get(projectId) ?? null
}

// ---------------------------------------------------------------------------
// Write
// ---------------------------------------------------------------------------

export async function refreshGit(projectId: string, projectPath?: string): Promise<void> {
  const path = projectPath ?? projectPaths.get(projectId)
  if (!path) return

  // Store the path for future refreshes
  projectPaths.set(projectId, path)

  try {
    const summary = await backend.git.getSummary(path)
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

export function clearGitState(projectId: string) {
  stopGitPolling(projectId)
  const next = new Map(gitSummaries)
  next.delete(projectId)
  gitSummaries = next
}
