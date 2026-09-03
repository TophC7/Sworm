import { backend } from '$lib/api/backend'
import { SvelteMap } from 'svelte/reactivity'

// Flat, ignore-aware file path cache for the surfaces that need to reach the
// whole project at once: the Quick Open palette and the sidebar's filter box.
// The explorer tree itself is lazy (see fileTree.svelte.ts) and does not use
// this list.
//
// Cached per (folder, showHidden) so the sidebar's hidden-files toggle and the
// palette's fixed view never invalidate each other.

type Entry = {
  paths: string[]
  truncated: boolean
  loading: boolean
  error: string | null
  loadedAt: number | null
}

const entries = new SvelteMap<string, Entry>()
const inflight = new Map<string, Promise<void>>()

function cacheKey(folderPath: string, showHidden: boolean): string {
  return `${folderPath}\u0000${showHidden ? 'hidden' : 'visible'}`
}

function getOrInit(key: string): Entry {
  let entry = entries.get(key)
  if (!entry) {
    entry = { paths: [], truncated: false, loading: false, error: null, loadedAt: null }
    entries.set(key, entry)
  }
  return entry
}

export function getProjectFilePaths(folderPath: string, showHidden = false): string[] {
  return entries.get(cacheKey(folderPath, showHidden))?.paths ?? []
}

export function isProjectFilesLoading(folderPath: string, showHidden = false): boolean {
  return entries.get(cacheKey(folderPath, showHidden))?.loading ?? false
}

/** True when the backend stopped short of listing every file. */
export function isProjectFilesTruncated(folderPath: string, showHidden = false): boolean {
  return entries.get(cacheKey(folderPath, showHidden))?.truncated ?? false
}

/**
 * True when a mutation has happened since this view was listed. Reading it
 * subscribes, so an effect that renders the list re-runs and refetches. A
 * failed walk is not stale: retrying it on every re-render would spin.
 */
export function isProjectFilesStale(folderPath: string, showHidden = false): boolean {
  const entry = entries.get(cacheKey(folderPath, showHidden))
  return entry !== undefined && entry.loadedAt === null && !entry.loading && entry.error === null
}

/**
 * Note that this folder's listing no longer matches disk without paying for a
 * new walk: one added file does not justify re-walking the project, and
 * nothing may be reading the list at all. The next reader refetches, because
 * `ensureProjectFiles` only trusts an entry with a `loadedAt`.
 */
export function markProjectFilesStale(folderPath: string): void {
  for (const showHidden of [false, true]) {
    const key = cacheKey(folderPath, showHidden)
    const entry = entries.get(key)
    if (entry) entries.set(key, { ...entry, loadedAt: null })
  }
}

/**
 * Ensure the cache for this folder is populated. Multiple concurrent
 * non-force callers share a single inflight request. A `force` call
 * placed while a non-force fetch is inflight chains a fresh fetch
 * after it — so the refresh button always observes post-mutation
 * state instead of the stale promise it was about to resolve with.
 */
export async function ensureProjectFiles(folderPath: string, showHidden = false, force = false): Promise<void> {
  const key = cacheKey(folderPath, showHidden)
  const entry = getOrInit(key)
  const existing = inflight.get(key)

  if (existing && !force) return existing
  if (!existing && !force && entry.loadedAt !== null) return

  entries.set(key, { ...entry, loading: true, error: null })

  const fetchOnce = async () => {
    try {
      const listed = await backend.files.listPaths(folderPath, showHidden)
      entries.set(key, {
        paths: listed.paths,
        truncated: listed.truncated,
        loading: false,
        error: null,
        loadedAt: Date.now()
      })
    } catch (e) {
      const prev = entries.get(key) ?? entry
      entries.set(key, {
        paths: prev.paths,
        truncated: prev.truncated,
        loading: false,
        error: e instanceof Error ? e.message : String(e),
        loadedAt: prev.loadedAt
      })
    }
  }

  let task: Promise<void>
  task = (existing ?? Promise.resolve()).then(fetchOnce).finally(() => {
    if (inflight.get(key) === task) inflight.delete(key)
  })
  inflight.set(key, task)
  return task
}

/** Force a reload of every cached view of this folder. */
export async function refreshProjectFiles(folderPath: string): Promise<void> {
  await Promise.all(
    [false, true]
      .filter((showHidden) => entries.has(cacheKey(folderPath, showHidden)))
      .map((showHidden) => ensureProjectFiles(folderPath, showHidden, true))
  )
}

export function releaseProjectFiles(folderPath: string): void {
  for (const showHidden of [false, true]) entries.delete(cacheKey(folderPath, showHidden))
}
