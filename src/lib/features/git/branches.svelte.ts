// Per-project branches state using Svelte 5 runes.
//
// Owns the Branches view list, paused-op status, preferences, recents,
// and dirty checkout handling. Preferences persist under the app_state
// key `branchesView:<projectId>`.

import { backend } from '$lib/api/backend'
import { getGitSummary, refreshGit } from '$lib/features/git/state.svelte'
import { createProjectKeyedStore } from '$lib/state/projectKeyedStore.svelte'
import type {
  BranchOpState,
  BranchSummary,
  GitSummary
} from '$lib/types/backend'

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

function prefsKey(projectId: string): string {
  return `branchesView:${projectId}`
}

interface ProjectEntry {
  list: BranchSummary[]
  opState: BranchOpState
  prefs: BranchesViewPrefs
  fetching: boolean
  fetchedThisSession: boolean
  lastFetchedAt: number | null
}

function freshEntry(): ProjectEntry {
  return {
    list: [],
    opState: 'idle',
    prefs: { ...DEFAULT_PREFS, recent: [] },
    fetching: false,
    fetchedThisSession: false,
    lastFetchedAt: null,
  }
}

// MODULE STATE //

const branchStore = createProjectKeyedStore<ProjectEntry>()

// Concurrent loadFor() calls coalesce into the same in-flight
// promise. Cleared on success or failure; re-entrancy from a
// different project id starts a fresh fetch.
const inFlightLoads = new Map<string, Promise<void>>()
const inFlightFetches = new Map<string, Promise<void>>()
const OP_STATE_POLL_MS = 1500

interface LoadOptions {
  autoFetch?: boolean
}

// READ ACCESSORS //

/** Read-only accessor; returns undefined for projects that haven't
 * been loaded yet. Callers should treat undefined as "loading" and
 * render a skeleton. */
export const byProject = {
  get(projectId: string): ProjectEntry | undefined {
    return branchStore.get(projectId)
  }
}

// PERSISTENCE //

function clonePrefs(prefs: BranchesViewPrefs): BranchesViewPrefs {
  return {
    layout: prefs.layout,
    sort: prefs.sort,
    showRemote: prefs.showRemote,
    recent: [...prefs.recent]
  }
}

async function loadPrefs(projectId: string): Promise<BranchesViewPrefs> {
  let raw: string | null
  try {
    raw = await backend.workspace.appStateGet(prefsKey(projectId))
  } catch {
    return { ...DEFAULT_PREFS, recent: [] }
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

async function persistPrefs(projectId: string, prefs: BranchesViewPrefs): Promise<void> {
  try {
    await backend.workspace.appStatePut(prefsKey(projectId), JSON.stringify(prefs))
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
  const recent = recentRaw
    .filter((s): s is string => typeof s === 'string' && s.length > 0)
    .slice(0, RECENT_CAP)
  return { layout, sort, showRemote, recent }
}

// ENTRY BOOKKEEPING //

function setEntry(projectId: string, entry: ProjectEntry) {
  branchStore.set(projectId, entry)
}

function patchEntry(projectId: string, patch: Partial<ProjectEntry>) {
  branchStore.patch(projectId, patch)
}

function patchPrefs(projectId: string, patch: Partial<BranchesViewPrefs>) {
  const current = branchStore.get(projectId)
  if (!current) return
  const nextPrefs = { ...current.prefs, ...patch }
  setEntry(projectId, { ...current, prefs: nextPrefs })
  void persistPrefs(projectId, clonePrefs(nextPrefs))
}

// LIFECYCLE //

/** Idempotent load. Concurrent callers join the same in-flight
 * promise. Subsequent calls after the first successful load return
 * immediately; use `refresh` to force a re-fetch. */
export function loadFor(
  projectId: string,
  projectPath: string,
  options: LoadOptions = {}
): Promise<void> {
  const autoFetch = options.autoFetch ?? true
  const existing = inFlightLoads.get(projectId)
  if (existing) {
    if (!autoFetch) return existing
    return existing.then(() => autoFetchOnce(projectId, projectPath))
  }
  if (branchStore.has(projectId)) {
    branchStore.setProjectPath(projectId, projectPath)
    if (autoFetch) void autoFetchOnce(projectId, projectPath)
    return Promise.resolve()
  }

  branchStore.setProjectPath(projectId, projectPath)
  const promise = (async () => {
    const prefs = await loadPrefs(projectId)
    let list: BranchSummary[] = []
    let opState: BranchOpState = 'idle'
    try {
      ;[list, opState] = await Promise.all([
        backend.git.branch.list(projectPath),
        backend.git.branch.status(projectPath)
      ])
    } catch (e) {
      console.error(`Failed to load branches for ${projectId}:`, e)
    }
    setEntry(projectId, {
      list,
      opState,
      prefs,
      fetching: false,
      fetchedThisSession: false,
      lastFetchedAt: null,
    })
    if (autoFetch) void autoFetchOnce(projectId, projectPath)
  })()
    .finally(() => {
      inFlightLoads.delete(projectId)
    })
  inFlightLoads.set(projectId, promise)
  return promise
}

/** Re-pull `branch.list` + `branch.status` and merge into the entry.
 * Prefs and transient flags survive. */
export async function refresh(projectId: string, projectPath?: string): Promise<void> {
  const path = branchStore.resolveProjectPath(projectId, projectPath)
  if (!path) return
  try {
    const [list, opState] = await Promise.all([
      backend.git.branch.list(path),
      backend.git.branch.status(path)
    ])
    patchEntry(projectId, { list, opState })
  } catch (e) {
    console.error(`Failed to refresh branches for ${projectId}:`, e)
  }
}

/** Drop an entry and stop any active op-state polling for it. */
export function clearFor(projectId: string) {
  inFlightLoads.delete(projectId)
  branchStore.clearFor(projectId)
}

// PREFS //

export function setLayout(projectId: string, layout: BranchLayout) {
  patchPrefs(projectId, { layout })
}

export function setSort(projectId: string, sort: BranchSort) {
  patchPrefs(projectId, { sort })
}

export function setShowRemote(projectId: string, showRemote: boolean) {
  patchPrefs(projectId, { showRemote })
}

/** Promote `name` to the front of the recents list, dedupe, cap at 10. */
export function markRecent(projectId: string, name: string) {
  const current = branchStore.get(projectId)
  if (!current || !name) return
  const filtered = current.prefs.recent.filter((r) => r !== name)
  const recent = [name, ...filtered].slice(0, RECENT_CAP)
  patchPrefs(projectId, { recent })
}

// FETCH STATE //

/** Mark that the Branches tab has completed its once-per-session fetch. */
export function markFetched(projectId: string) {
  patchEntry(projectId, { fetching: false, fetchedThisSession: true, lastFetchedAt: Date.now() })
}

export function fetchBranches(projectId: string, projectPath: string): Promise<void> {
  const existing = inFlightFetches.get(projectId)
  if (existing) return existing

  const current = branchStore.get(projectId)
  if (!current) return Promise.resolve()

  patchEntry(projectId, { fetching: true, fetchedThisSession: true })
  const promise = (async () => {
    try {
      await backend.git.fetch(projectPath)
      await refresh(projectId, projectPath)
      markFetched(projectId)
    } catch (e) {
      patchEntry(projectId, { fetching: false })
      throw e
    }
  })()
    .finally(() => {
      inFlightFetches.delete(projectId)
    })
  inFlightFetches.set(projectId, promise)
  return promise
}

async function autoFetchOnce(projectId: string, projectPath: string) {
  const current = branchStore.get(projectId)
  if (!current || current.fetchedThisSession) return
  try {
    await fetchBranches(projectId, projectPath)
  } catch (e) {
    console.error(`Failed to auto-fetch branches for ${projectId}:`, e)
  }
}

// DIRTY CHECKOUT //

/**
 * Force-refresh the git summary, then return whether the working
 * tree (or index, or untracked set) holds any uncommitted state.
 * Callers should treat `true` as "must route through CheckoutDialog
 * before invoking checkout."
 */
export async function dirtyCheck(
  projectId: string,
  projectPath: string
): Promise<{ dirty: boolean; summary: GitSummary | null }> {
  await refreshGit(projectId, projectPath)
  const summary = getGitSummary(projectId)
  if (!summary || !summary.is_repo) return { dirty: false, summary }
  const dirty =
    summary.staged_count > 0 ||
    summary.unstaged_count > 0 ||
    summary.untracked_count > 0
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
  return (
    typeof err === 'object' &&
    err !== null &&
    'kind' in err &&
    (err as { kind?: unknown }).kind === 'dirtyWorktree'
  )
}

/**
 * Switch to `name` only when the working tree is clean. On dirty
 * trees, throws `DirtyCheckoutError` so the caller can open
 * `CheckoutDialog` and decide between stash-and-switch and cancel.
 * This is the only non-dialog checkout path; successful checkouts
 * refresh branch state and persist recents immediately.
 */
export async function safeCheckout(
  projectId: string,
  projectPath: string,
  name: string
): Promise<void> {
  const { dirty, summary } = await dirtyCheck(projectId, projectPath)
  if (dirty) {
    throw new DirtyCheckoutError(summary)
  }
  try {
    await backend.git.branch.checkout(projectPath, name)
  } catch (e) {
    if (isBackendDirtyWorktreeError(e)) {
      await refreshGit(projectId, projectPath)
      throw new DirtyCheckoutError(getGitSummary(projectId))
    }
    throw e
  }
  markRecent(projectId, name)
  await Promise.all([refresh(projectId, projectPath), refreshGit(projectId, projectPath)])
}

export async function safeCheckoutRemoteAsLocal(
  projectId: string,
  projectPath: string,
  remoteName: string,
  localName: string
): Promise<void> {
  const { dirty, summary } = await dirtyCheck(projectId, projectPath)
  if (dirty) {
    throw new DirtyCheckoutError(summary)
  }
  try {
    await backend.git.branch.checkoutRemoteAsLocal(projectPath, remoteName, localName)
  } catch (e) {
    if (isBackendDirtyWorktreeError(e)) {
      await refreshGit(projectId, projectPath)
      throw new DirtyCheckoutError(getGitSummary(projectId))
    }
    throw e
  }
  markRecent(projectId, localName)
  await Promise.all([refresh(projectId, projectPath), refreshGit(projectId, projectPath)])
}

// PAUSED OP POLLING //

/**
 * Poll `branch.status` every 1500ms while a component observes a
 * non-idle op state. Multiple consumers share one interval and each
 * releases through `stopOpStatePolling`.
 */
export function pollOpState(projectId: string, projectPath: string) {
  branchStore.startPolling(projectId, projectPath, {
    intervalMs: OP_STATE_POLL_MS,
    tick: async (id, path) => {
      try {
        const next = await backend.git.branch.status(path)
        if (branchStore.get(id)?.opState !== next) {
          patchEntry(id, { opState: next })
        }
      } catch (e) {
        console.error('opState poll failed:', e)
        branchStore.stopAllPolling(id)
      }
    }
  })
}

export function stopOpStatePolling(projectId: string) {
  branchStore.stopPolling(projectId)
}
