// Folder-keyed branches state using Svelte 5 runes.
//
// Owns the Branches view list, paused-op status, preferences, recents,
// and dirty checkout handling. Preferences persist per window and folder
// under the app_state key `branchesView:<windowLabel>:<folderPath>`.

import { backend } from '$lib/api/backend'
import { getGitSummary, onRepoRefresh, refreshGit, runGitAction } from '$lib/features/git/state.svelte'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import { getWindowLabel } from '$lib/features/workbench/state.svelte'
import type { BranchOpState, BranchSummary, GitSummary } from '$lib/types/backend'

// TYPES //

export type BranchLayout = 'list' | 'tree'
export type BranchSort = 'date' | 'alpha'

export interface BranchesViewPrefs {
  layout: BranchLayout
  sort: BranchSort
  showRemote: boolean
  /** Most-recently checked-out local branches; capped at 10. */
  recent: string[]
}

const DEFAULT_PREFS: BranchesViewPrefs = {
  layout: 'tree',
  sort: 'date',
  showRemote: false,
  recent: []
}

const RECENT_CAP = 10

interface FolderEntry {
  list: BranchSummary[]
  opState: BranchOpState
  prefs: BranchesViewPrefs
  fetching: boolean
  fetchedThisSession: boolean
  lastFetchedAt: number | null
}

// MODULE STATE //

const branchStore = createFolderKeyedStore<FolderEntry>()

// Concurrent loadFor() calls coalesce into the same in-flight
// promise. Cleared on success or failure; re-entrancy from a
// different folder starts a fresh fetch.
const inFlightLoads = new Map<string, Promise<void>>()
const inFlightFetches = new Map<string, Promise<void>>()
const folderGenerations = new Map<string, number>()

function folderGeneration(folderPath: string): number {
  return folderGenerations.get(folderPath) ?? 0
}
const OP_STATE_POLL_MS = 1500

interface LoadOptions {
  autoFetch?: boolean
}

// READ ACCESSORS //

/** Read-only accessor; returns undefined for folders that haven't
 * been loaded yet. Callers should treat undefined as "loading" and
 * render a skeleton. */
export const byFolder = {
  get(folderPath: string): FolderEntry | undefined {
    return branchStore.get(folderPath)
  }
}

/** Forget the folder's branch state and stop op-state polling; called when the workbench releases the folder. */
export function releaseBranchFolder(folderPath: string) {
  folderGenerations.set(folderPath, folderGeneration(folderPath) + 1)
  branchStore.delete(folderPath)
  inFlightLoads.delete(folderPath)
  inFlightFetches.delete(folderPath)
}

// PERSISTENCE //

async function loadPrefs(folderPath: string): Promise<BranchesViewPrefs> {
  const key = `branchesView:${getWindowLabel()}:${folderPath}`
  let raw: string | null
  try {
    raw = await backend.app.stateGet(key)
  } catch {
    return { ...DEFAULT_PREFS, recent: [] }
  }
  if (!raw) {
    const legacyKey = `branchesView:${folderPath}`
    try {
      raw = await backend.app.stateGet(legacyKey)
      if (raw) {
        await backend.app.statePut(key, raw)
        await backend.app.stateDelete(legacyKey)
      }
    } catch (error) {
      console.warn('Failed to migrate branchesView prefs:', error)
    }
  }
  if (!raw) return { ...DEFAULT_PREFS, recent: [] }
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return { ...DEFAULT_PREFS, recent: [] }
  }
  return extractPrefs(parsed)
}

async function persistPrefs(folderPath: string, prefs: BranchesViewPrefs): Promise<void> {
  try {
    await backend.app.statePut(`branchesView:${getWindowLabel()}:${folderPath}`, JSON.stringify(prefs))
  } catch (e) {
    console.error('Failed to persist branchesView prefs:', e)
  }
}

function extractPrefs(blob: unknown): BranchesViewPrefs {
  if (!blob || typeof blob !== 'object' || Array.isArray(blob)) {
    return { ...DEFAULT_PREFS, recent: [] }
  }
  const v = blob as Record<string, unknown>
  const layout: BranchLayout = v.layout === 'list' ? 'list' : 'tree'
  const sort: BranchSort = v.sort === 'alpha' ? 'alpha' : 'date'
  const showRemote = typeof v.showRemote === 'boolean' ? v.showRemote : false
  const recentRaw = Array.isArray(v.recent) ? v.recent : []
  const recent = recentRaw.filter((s): s is string => typeof s === 'string' && s.length > 0).slice(0, RECENT_CAP)
  return { layout, sort, showRemote, recent }
}

// ENTRY BOOKKEEPING //

function patchPrefs(folderPath: string, patch: Partial<BranchesViewPrefs>) {
  const current = branchStore.get(folderPath)
  if (!current) return
  const nextPrefs = { ...current.prefs, ...patch }
  branchStore.set(folderPath, { ...current, prefs: nextPrefs })
  void persistPrefs(folderPath, { ...nextPrefs, recent: [...nextPrefs.recent] })
}

// LIFECYCLE //

/** Idempotent load. Concurrent callers join the same in-flight
 * promise. Subsequent calls after the first successful load return
 * immediately; use `refresh` to force a re-fetch. */
export function loadFor(folderPath: string, options: LoadOptions = {}): Promise<void> {
  const autoFetch = options.autoFetch ?? true
  const generation = folderGeneration(folderPath)
  const existing = inFlightLoads.get(folderPath)
  if (existing) {
    if (!autoFetch) return existing
    return existing.then(() => autoFetchOnce(folderPath))
  }
  if (branchStore.has(folderPath)) {
    if (autoFetch) void autoFetchOnce(folderPath)
    return Promise.resolve()
  }

  let promise: Promise<void>
  promise = (async () => {
    const prefs = await loadPrefs(folderPath)
    let list: BranchSummary[] = []
    let opState: BranchOpState = 'idle'
    try {
      ;[list, opState] = await Promise.all([backend.git.branch.list(folderPath), backend.git.branch.status(folderPath)])
    } catch (e) {
      console.error(`Failed to load branches for ${folderPath}:`, e)
    }
    if (folderGeneration(folderPath) !== generation) return
    branchStore.set(folderPath, {
      list,
      opState,
      prefs,
      fetching: false,
      fetchedThisSession: false,
      lastFetchedAt: null
    })
    if (autoFetch) void autoFetchOnce(folderPath)
  })().finally(() => {
    if (inFlightLoads.get(folderPath) === promise) inFlightLoads.delete(folderPath)
  })
  inFlightLoads.set(folderPath, promise)
  return promise
}

/** Re-pull `branch.list` + `branch.status` and merge into the entry.
 * Prefs and transient flags survive. Runs as part of every full repo
 * refresh (see `onRepoRefresh` below); not called directly by feature code. */
export async function refresh(folderPath: string): Promise<void> {
  const generation = folderGeneration(folderPath)
  try {
    const [list, opState] = await Promise.all([
      backend.git.branch.list(folderPath),
      backend.git.branch.status(folderPath)
    ])
    if (folderGeneration(folderPath) !== generation) return
    branchStore.patch(folderPath, { list, opState })
  } catch (e) {
    console.error(`Failed to refresh branches for ${folderPath}:`, e)
  }
}

onRepoRefresh((folderPath) => (branchStore.has(folderPath) ? refresh(folderPath) : undefined))

// PREFS //

export function setLayout(folderPath: string, layout: BranchLayout) {
  patchPrefs(folderPath, { layout })
}

export function setSort(folderPath: string, sort: BranchSort) {
  patchPrefs(folderPath, { sort })
}

export function setShowRemote(folderPath: string, showRemote: boolean) {
  patchPrefs(folderPath, { showRemote })
}

/** Promote `name` to the front of the recents list, dedupe, cap at 10. */
export function markRecent(folderPath: string, name: string) {
  const current = branchStore.get(folderPath)
  if (!current || !name) return
  const filtered = current.prefs.recent.filter((r) => r !== name)
  const recent = [name, ...filtered].slice(0, RECENT_CAP)
  patchPrefs(folderPath, { recent })
}

// FETCH STATE //

export function fetchBranches(folderPath: string): Promise<void> {
  const generation = folderGeneration(folderPath)
  const existing = inFlightFetches.get(folderPath)
  if (existing) return existing

  const current = branchStore.get(folderPath)
  if (!current) return Promise.resolve()

  branchStore.patch(folderPath, { fetching: true, fetchedThisSession: true })
  let promise: Promise<void>
  promise = (async () => {
    try {
      await runGitAction(folderPath, (path) => backend.git.fetch(path))
      if (folderGeneration(folderPath) !== generation) return
      branchStore.patch(folderPath, { fetching: false, fetchedThisSession: true, lastFetchedAt: Date.now() })
    } catch (e) {
      if (folderGeneration(folderPath) === generation) {
        branchStore.patch(folderPath, { fetching: false })
      }
      throw e
    }
  })().finally(() => {
    if (inFlightFetches.get(folderPath) === promise) inFlightFetches.delete(folderPath)
  })
  inFlightFetches.set(folderPath, promise)
  return promise
}

async function autoFetchOnce(folderPath: string) {
  const current = branchStore.get(folderPath)
  if (!current || current.fetchedThisSession) return
  try {
    await fetchBranches(folderPath)
  } catch (e) {
    console.error(`Failed to auto-fetch branches for ${folderPath}:`, e)
  }
}

// DIRTY CHECKOUT //

/**
 * Force-refresh the git summary, then return whether the working
 * tree (or index, or untracked set) holds any uncommitted state.
 * Callers should treat `true` as "must route through CheckoutDialog
 * before invoking checkout."
 */
async function dirtyCheck(folderPath: string): Promise<{ dirty: boolean; summary: GitSummary | null }> {
  await refreshGit(folderPath)
  const summary = getGitSummary(folderPath)
  if (!summary || !summary.is_repo) return { dirty: false, summary }
  const dirty = summary.staged_count > 0 || summary.unstaged_count > 0 || summary.untracked_count > 0
  return { dirty, summary }
}

/** Tagged error thrown by `safeCheckout` when the working tree is
 * dirty so the UI can route to `CheckoutDialog`. */
export class DirtyCheckoutError extends Error {
  readonly kind = 'dirty' as const
  readonly summary: GitSummary | null
  constructor(summary: GitSummary | null) {
    super('Working tree is dirty')
    this.summary = summary
  }
}

export function isDirtyCheckoutError(err: unknown): err is DirtyCheckoutError {
  return err instanceof DirtyCheckoutError
}

interface BackendDirtyWorktreeError {
  kind: 'dirtyWorktree'
  message: string
}

function isBackendDirtyWorktreeError(err: unknown): err is BackendDirtyWorktreeError {
  return typeof err === 'object' && err !== null && 'kind' in err && err.kind === 'dirtyWorktree'
}

/**
 * Run `checkout` only when the working tree is clean. On dirty trees,
 * throws `DirtyCheckoutError` so the caller can open `CheckoutDialog`
 * and decide between stash-and-switch and cancel. Successful checkouts
 * refresh branch state and persist recents immediately.
 */
async function guardedCheckout(
  folderPath: string,
  recentName: string,
  checkout: (path: string) => Promise<void>
): Promise<void> {
  const { dirty, summary } = await dirtyCheck(folderPath)
  if (dirty) {
    throw new DirtyCheckoutError(summary)
  }
  try {
    await runGitAction(folderPath, checkout)
  } catch (e) {
    // runGitAction already refreshed, so the summary reflects the tree that blocked the switch.
    if (isBackendDirtyWorktreeError(e)) throw new DirtyCheckoutError(getGitSummary(folderPath))
    throw e
  }
  markRecent(folderPath, recentName)
}

/** Switch to local branch `name`; see `guardedCheckout` for dirty-tree handling. */
export function safeCheckout(folderPath: string, name: string): Promise<void> {
  return guardedCheckout(folderPath, name, (path) => backend.git.branch.checkout(path, name))
}

/** Create `localName` tracking `remoteName` and switch to it; see `guardedCheckout`. */
export function safeCheckoutRemoteAsLocal(folderPath: string, remoteName: string, localName: string): Promise<void> {
  return guardedCheckout(folderPath, localName, (path) =>
    backend.git.branch.checkoutRemoteAsLocal(path, remoteName, localName)
  )
}

// PAUSED OP POLLING //

/**
 * Poll `branch.status` every 1500ms while a component observes a
 * non-idle op state. Multiple consumers share one interval and each
 * releases through `stopOpStatePolling`.
 */
export function pollOpState(folderPath: string) {
  branchStore.startPolling(folderPath, {
    intervalMs: OP_STATE_POLL_MS,
    tick: async (path) => {
      try {
        const next = await backend.git.branch.status(path)
        if (branchStore.get(path)?.opState !== next) {
          branchStore.patch(path, { opState: next })
        }
      } catch (e) {
        console.error('opState poll failed:', e)
        branchStore.stopAllPolling(path)
      }
    }
  })
}

export function stopOpStatePolling(folderPath: string) {
  branchStore.stopPolling(folderPath)
}
