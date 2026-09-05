// Recent folders — canonical folder paths managed and broadcast by the backend.
// Feeds the empty state, the hamburger menu's "Open Recent" group, and the
// activity map's discovered-folder filter.

import { backend } from '$lib/api/backend'

let recentFolders = $state<string[]>([])
let recentFoldersListening = false

export function getRecentFolders(): string[] {
  return recentFolders
}

/** Probe `paths` against the backend and keep those that still resolve. */
export async function filterExistingFolders(paths: string[]): Promise<string[]> {
  const checks = await Promise.allSettled(paths.map((path) => backend.folders.resolve(path)))
  return paths.filter((_, i) => checks[i].status === 'fulfilled')
}

/** Load the MRU and drop entries whose folder no longer resolves. */
export async function loadRecentFolders(): Promise<void> {
  const saved = await backend.folders.recentList()
  const alive = await filterExistingFolders(saved)
  recentFolders = alive
  const missing = saved.filter((p) => !alive.includes(p))
  if (missing.length > 0) {
    void backend.folders.recentRemove(missing)
  }
  if (!recentFoldersListening) {
    recentFoldersListening = true
    void backend.folders.onRecentFoldersChanged((folders) => {
      recentFolders = folders
    })
  }
}

/** Move `path` (already canonical) to the front of the MRU and persist immediately. */
export function pushRecentFolder(path: string): void {
  recentFolders = [path, ...recentFolders.filter((p) => p !== path)]
  void backend.folders.recentTouch(path).catch((e) => console.warn('Failed to touch recent folder:', e))
}
