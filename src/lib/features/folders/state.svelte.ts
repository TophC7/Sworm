// Recent folders — canonical folder paths opened in Sworm, managed and
// broadcast by the backend. Feeds Home's project cards, the "Open Recent"
// menus, and the folder switcher's starting point.

import { backend } from '$lib/api/backend'
import type { RecentFolder } from '$lib/types/backend'

let recentFolders = $state<RecentFolder[]>([])
let recentFoldersListening = false

export function getRecentFolders(): RecentFolder[] {
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
  const alive = new Set(await filterExistingFolders(saved.map((folder) => folder.path)))
  recentFolders = saved.filter((folder) => alive.has(folder.path))
  const missing = saved.filter((folder) => !alive.has(folder.path)).map((folder) => folder.path)
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
  recentFolders = [
    { path, opened_at: new Date().toISOString() },
    ...recentFolders.filter((folder) => folder.path !== path)
  ]
  void backend.folders.recentTouch(path).catch((e) => console.warn('Failed to touch recent folder:', e))
}
