import { backend } from '$lib/api/backend'
import { SvelteMap } from 'svelte/reactivity'

// Per-folder file path cache used by surfaces that need a flat
// folder-wide file list (Quick Open palette, future content search).
// Centralized so we fetch once per folder and refresh from a single
// place when files mutate (create/rename/delete in the tree, manual
// refresh).

type Entry = {
  paths: string[]
  loading: boolean
  error: string | null
  loadedAt: number | null
}

const entries = new SvelteMap<string, Entry>()
const inflight = new Map<string, Promise<void>>()

function emptyEntry(): Entry {
  return { paths: [], loading: false, error: null, loadedAt: null }
}

function getOrInit(folderPath: string): Entry {
  let entry = entries.get(folderPath)
  if (!entry) {
    entry = emptyEntry()
    entries.set(folderPath, entry)
  }
  return entry
}

export function getProjectFilePaths(folderPath: string): string[] {
  return entries.get(folderPath)?.paths ?? []
}

export function isProjectFilesLoading(folderPath: string): boolean {
  return entries.get(folderPath)?.loading ?? false
}

/**
 * Ensure the cache for this folder is populated. Multiple concurrent
 * non-force callers share a single inflight request. A `force` call
 * placed while a non-force fetch is inflight chains a fresh fetch
 * after it — so the refresh button always observes post-mutation
 * state instead of the stale promise it was about to resolve with.
 */
export async function ensureProjectFiles(folderPath: string, force = false): Promise<void> {
  const entry = getOrInit(folderPath)
  const existing = inflight.get(folderPath)

  if (existing && !force) return existing
  if (!existing && !force && entry.loadedAt !== null) return

  entries.set(folderPath, { ...entry, loading: true, error: null })

  const fetchOnce = async () => {
    try {
      const paths = await backend.files.listAll(folderPath)
      entries.set(folderPath, {
        paths,
        loading: false,
        error: null,
        loadedAt: Date.now()
      })
    } catch (e) {
      const prev = entries.get(folderPath) ?? entry
      entries.set(folderPath, {
        paths: prev.paths,
        loading: false,
        error: e instanceof Error ? e.message : String(e),
        loadedAt: prev.loadedAt
      })
    }
  }

  let task: Promise<void>
  task = (existing ?? Promise.resolve()).then(fetchOnce).finally(() => {
    if (inflight.get(folderPath) === task) inflight.delete(folderPath)
  })
  inflight.set(folderPath, task)
  return task
}

/** Force a reload; callers can await to know when the new data lands. */
export function refreshProjectFiles(folderPath: string): Promise<void> {
  return ensureProjectFiles(folderPath, true)
}
