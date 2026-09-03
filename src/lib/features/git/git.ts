import type { GitChange, GitSummary } from '$lib/types/backend'

/** Parse a stash message ("On <branch>: <msg>" or "WIP on <branch>: <msg>")
 *  into its branch and description parts. */
export function parseStashMessage(raw: string): { branch: string | null; label: string } {
  const match = raw.match(/^(?:WIP )?[Oo]n ([^:]+):\s*(.*)$/)
  if (match) {
    return { branch: match[1], label: match[2] || 'WIP' }
  }
  return { branch: null, label: raw }
}

// OPTIMISTIC SUMMARY PATCHES //
//
// Predict what `git status` will report after a stage / unstage / discard so
// the working tree moves the instant the user acts; the confirming refresh
// corrects line counts and any edge the prediction gets wrong. `files === null`
// means every entry on the relevant side. Counting rules match
// `get_summary` in `src-tauri/src/services/git.rs`.

function matches(paths: Set<string> | null, change: GitChange): boolean {
  return paths === null || paths.has(change.path)
}

function recount(summary: GitSummary, changes: GitChange[]): GitSummary {
  let stagedCount = 0
  let unstagedCount = 0
  let untrackedCount = 0
  for (const change of changes) {
    if (change.staged) stagedCount += 1
    if (!change.staged && change.status !== '?') unstagedCount += 1
    if (change.status === '?') untrackedCount += 1
  }
  return {
    ...summary,
    changes,
    staged_count: stagedCount,
    unstaged_count: unstagedCount,
    untracked_count: untrackedCount
  }
}

/** Unstaged entries become staged; a path already staged collapses into one entry. */
export function stageChanges(summary: GitSummary, files: string[] | null): GitSummary {
  const paths = files === null ? null : new Set(files)
  const staging = new Set<string>()
  for (const change of summary.changes) {
    if (!change.staged && matches(paths, change)) staging.add(change.path)
  }
  const changes = summary.changes
    .filter((change) => !(change.staged && staging.has(change.path)))
    .map((change) =>
      !change.staged && staging.has(change.path)
        ? { ...change, staged: true, status: change.status === '?' ? 'A' : change.status }
        : change
    )
  return recount(summary, changes)
}

/** Staged entries return to the working tree; a path also unstaged just drops its staged copy. */
export function unstageChanges(summary: GitSummary, files: string[] | null): GitSummary {
  const paths = files === null ? null : new Set(files)
  const unstaged = new Set<string>()
  for (const change of summary.changes) {
    if (!change.staged) unstaged.add(change.path)
  }
  const changes = summary.changes
    .filter((change) => !(change.staged && matches(paths, change) && unstaged.has(change.path)))
    .map((change) =>
      change.staged && matches(paths, change)
        ? { ...change, staged: false, status: change.status === 'A' ? '?' : change.status }
        : change
    )
  return recount(summary, changes)
}

/** Unstaged entries (untracked included) disappear; staged entries are untouched. */
export function discardChanges(summary: GitSummary, files: string[] | null): GitSummary {
  const paths = files === null ? null : new Set(files)
  return recount(
    summary,
    summary.changes.filter((change) => change.staged || !matches(paths, change))
  )
}
