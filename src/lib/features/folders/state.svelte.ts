// Recent folders — bounded MRU of canonical folder paths, persisted under
// the `recent_folders` app-state key. Feeds the empty state, the hamburger
// menu's "Open Recent" group, and the activity map's discovered-folder filter.

import { backend } from '$lib/api/backend'

const APP_STATE_KEY_RECENT_FOLDERS = 'recent_folders'
const MAX_RECENT_FOLDERS = 12

let recentFolders = $state<string[]>([])

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
  let saved: string[] = []
  try {
    const raw = await backend.app.stateGet(APP_STATE_KEY_RECENT_FOLDERS)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    saved = Array.isArray(parsed) ? parsed.filter((p): p is string => typeof p === 'string') : []
  } catch (error) {
    console.warn('Recent folders restore failed:', error)
  }
  const alive = await filterExistingFolders(saved)
  recentFolders = alive
  if (alive.length !== saved.length) void persist()
}

/** Move `path` (already canonical) to the front of the MRU and persist immediately. */
export function pushRecentFolder(path: string): void {
  if (recentFolders[0] === path) return
  recentFolders = [path, ...recentFolders.filter((p) => p !== path)].slice(0, MAX_RECENT_FOLDERS)
  void persist()
}

async function persist(): Promise<void> {
  try {
    await backend.app.statePut(APP_STATE_KEY_RECENT_FOLDERS, JSON.stringify(recentFolders))
  } catch (error) {
    console.warn('Recent folders persist failed:', error)
  }
}
