import { backend } from '$lib/api/backend'
import { SvelteMap } from 'svelte/reactivity'

// Per-project file path cache used by surfaces that need a flat
// project-wide file list (Quick Open palette, future content search).
// Centralized so we fetch once per project and invalidate from a
// single place when files mutate (create/rename/delete in the tree,
// manual refresh, project switch).

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

function getOrInit(projectId: string): Entry {
  let entry = entries.get(projectId)
  if (!entry) {
    entry = emptyEntry()
    entries.set(projectId, entry)
  }
  return entry
}

export function getProjectFilePaths(projectId: string): string[] {
  return entries.get(projectId)?.paths ?? []
}

export function isProjectFilesLoading(projectId: string): boolean {
  return entries.get(projectId)?.loading ?? false
}

/**
 * Ensure the cache for this project is populated. Multiple concurrent
 * non-force callers share a single inflight request. A `force` call
 * placed while a non-force fetch is inflight chains a fresh fetch
 * after it — so the refresh button always observes post-mutation
 * state instead of the stale promise it was about to resolve with.
 */
export async function ensureProjectFiles(projectId: string, projectPath: string, force = false): Promise<void> {
  const entry = getOrInit(projectId)
  const existing = inflight.get(projectId)

  if (existing && !force) return existing
  if (!existing && !force && entry.loadedAt !== null) return

  entries.set(projectId, { ...entry, loading: true, error: null })

  const fetchOnce = async () => {
    try {
      const paths = await backend.files.listAll(projectPath)
      entries.set(projectId, {
        paths,
        loading: false,
        error: null,
        loadedAt: Date.now()
      })
    } catch (e) {
      const prev = entries.get(projectId) ?? entry
      entries.set(projectId, {
        paths: prev.paths,
        loading: false,
        error: e instanceof Error ? e.message : String(e),
        loadedAt: prev.loadedAt
      })
    }
  }

  let task: Promise<void>
  task = (existing ?? Promise.resolve()).then(fetchOnce).finally(() => {
    if (inflight.get(projectId) === task) inflight.delete(projectId)
  })
  inflight.set(projectId, task)
  return task
}

/** Force a reload; callers can await to know when the new data lands. */
export function refreshProjectFiles(projectId: string, projectPath: string): Promise<void> {
  return ensureProjectFiles(projectId, projectPath, true)
}

/** Drop the cache for a project (e.g. on project close). */
export function invalidateProjectFiles(projectId: string): void {
  entries.delete(projectId)
  inflight.delete(projectId)
}
