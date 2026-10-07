// Folder-keyed git state module using Svelte 5 runes.
//
// One entry per open folder holds everything the git sidebar and status bar
// read: the working-tree summary, the commit graph, and the stash count/list.
// Freshness comes from backend events, in-app mutation refreshes, window-focus
// refreshes, and a cheap active-folder reconciliation poll. All summary reads
// funnel through the same coalescing queue so slow `git status` calls never
// overlap or build a backlog.
//
// Two guards keep concurrent reads from fighting a mutation:
//   - `epoch`: bumped by `runGitAction` when a mutation lands; a summary read
//     that started before the bump is discarded on arrival.
//   - `lastApplied`: a summary read never overwrites one that started later.
// Without them a slow pre-mutation `git status` lands after the post-mutation
// one and the working tree visibly bounces.

import { backend } from '$lib/api/backend'
import { withTrashFallback } from '$lib/features/files/trash'
import { discardChanges, stageChanges, unstageChanges } from '$lib/features/git/summaryPatches'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import { getErrorMessage } from '$lib/utils/client-error'
import type { GitSummary, GraphCommit, StashEntry } from '$lib/types/backend'
import { platform } from '$lib/platform'

const GRAPH_LIMIT = 100
const RECONCILE_INTERVAL_MS = 2_000

export interface GitFreshness {
  readError: string | null
  watchError: string | null
}

interface RepoState extends GitFreshness {
  summary: GitSummary | null
  /** null = never loaded; loaded once the graph is on screen. */
  graph: GraphCommit[] | null
  stashCount: number | null
  /** null = never loaded; the list is costlier than the count, so only the stash tab loads it. */
  stashes: StashEntry[] | null
}

export type RefreshScope = 'summary' | 'all'

const gitStore = createFolderKeyedStore<RepoState>()

interface RefreshQueue {
  pendingScope: RefreshScope | null
  promise: Promise<void>
}

interface FolderRuntime {
  epoch: number
  lastApplied: number
  inFlightActions: number
  pendingActionScope: RefreshScope | null
  loads: Map<'graph' | 'stashes' | 'watch', Promise<void>>
  refreshQueue: RefreshQueue | null
  reconciliation: Promise<void> | null
  activeRefs: number
  activeReady: Promise<void> | null
}

const runtimes = new Map<string, FolderRuntime>()
let nextSeq = 0

function ensureEntry(folderPath: string) {
  if (!gitStore.has(folderPath)) {
    gitStore.set(folderPath, {
      summary: null,
      graph: null,
      stashCount: null,
      stashes: null,
      readError: null,
      watchError: null
    })
    runtimes.set(folderPath, {
      epoch: 0,
      lastApplied: 0,
      inFlightActions: 0,
      pendingActionScope: null,
      loads: new Map(),
      refreshQueue: null,
      reconciliation: null,
      activeRefs: 0,
      activeReady: null
    })
  }
}

function summariesEqual(a: GitSummary | null | undefined, b: GitSummary): boolean {
  if (!a) return false
  if (
    a.is_repo !== b.is_repo ||
    a.branch !== b.branch ||
    a.base_ref !== b.base_ref ||
    a.ahead !== b.ahead ||
    a.behind !== b.behind ||
    a.staged_count !== b.staged_count ||
    a.unstaged_count !== b.unstaged_count ||
    a.untracked_count !== b.untracked_count ||
    a.changes.length !== b.changes.length
  ) {
    return false
  }

  for (let i = 0; i < a.changes.length; i++) {
    const left = a.changes[i]
    const right = b.changes[i]
    if (
      left.path !== right.path ||
      left.status !== right.status ||
      left.staged !== right.staged ||
      left.additions !== right.additions ||
      left.deletions !== right.deletions
    ) {
      return false
    }
  }

  return true
}

/** Same commits with the same ref decorations; a refresh that changes neither keeps the rendered rows. */
function graphsEqual(a: GraphCommit[] | null, b: GraphCommit[]): boolean {
  if (!a || a.length !== b.length) return false
  for (let i = 0; i < a.length; i++) {
    if (a[i].hash !== b[i].hash || a[i].refs.length !== b[i].refs.length) return false
    for (let ref = 0; ref < a[i].refs.length; ref++) {
      if (a[i].refs[ref] !== b[i].refs[ref]) return false
    }
  }
  return true
}

// READ //
export function getGitSummary(folderPath: string): GitSummary | null {
  return gitStore.get(folderPath)?.summary ?? null
}

const healthyFreshness: GitFreshness = { readError: null, watchError: null }

export function getGitFreshness(folderPath: string): GitFreshness {
  return gitStore.get(folderPath) ?? healthyFreshness
}

export function getGitGraph(folderPath: string): GraphCommit[] | null {
  return gitStore.get(folderPath)?.graph ?? null
}

export function getStashCount(folderPath: string): number {
  return gitStore.get(folderPath)?.stashCount ?? 0
}

export function getStashes(folderPath: string): StashEntry[] | null {
  return gitStore.get(folderPath)?.stashes ?? null
}

// REFRESH //
async function refreshGitNow(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return
  const epoch = rt.epoch
  const seq = ++nextSeq
  try {
    const summary = await backend.git.getSummary(folderPath)
    // A mutation landed meanwhile; its own refresh follows.
    if (runtimes.get(folderPath) !== rt || rt.epoch !== epoch) return
    // A read that started later already settled.
    if (rt.lastApplied > seq) return
    rt.lastApplied = seq
    const patch: Partial<RepoState> = { readError: null }
    if (!summariesEqual(getGitSummary(folderPath), summary)) patch.summary = summary
    // Non-repositories have nothing to watch; never report that expected state
    // as degraded freshness.
    if (!summary.is_repo) patch.watchError = null
    gitStore.patch(folderPath, patch)
  } catch (error) {
    if (
      runtimes.get(folderPath) !== rt ||
      rt.epoch !== epoch ||
      rt.lastApplied > seq
    ) {
      return
    }
    rt.lastApplied = seq
    gitStore.patch(folderPath, { readError: getErrorMessage(error) })
    console.error(`Failed to refresh git for ${folderPath}:`, error)
  }
}

async function refreshGraph(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return
  try {
    const graph = await backend.git.getGraph(folderPath, GRAPH_LIMIT)
    if (runtimes.get(folderPath) !== rt) return
    if (graphsEqual(getGitGraph(folderPath), graph)) return
    gitStore.patch(folderPath, { graph })
  } catch (e) {
    console.error(`Failed to load git graph for ${folderPath}:`, e)
  }
}

async function refreshStashCount(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return
  try {
    const stashCount = await backend.git.stashCount(folderPath)
    if (runtimes.get(folderPath) !== rt) return
    gitStore.patch(folderPath, { stashCount })
  } catch (e) {
    console.error(`Failed to count stashes for ${folderPath}:`, e)
  }
}

async function refreshStashes(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return
  try {
    const stashes = await backend.git.stashList(folderPath)
    if (runtimes.get(folderPath) !== rt) return
    gitStore.patch(folderPath, { stashes, stashCount: stashes.length })
  } catch (e) {
    console.error(`Failed to list stashes for ${folderPath}:`, e)
  }
}

/** Coalesce a first load per folder+kind; later callers join the in-flight promise. */
function loadOnce(
  rt: FolderRuntime,
  kind: 'graph' | 'stashes' | 'watch',
  load: () => Promise<void>
): Promise<void> {
  const existing = rt.loads.get(kind)
  if (existing) return existing
  const promise = load().finally(() => {
    if (rt.loads.get(kind) === promise) rt.loads.delete(kind)
  })
  rt.loads.set(kind, promise)
  return promise
}

/** Load the graph if it was never loaded; refreshes keep it current afterwards. */
export function loadGraph(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt || getGitGraph(folderPath)) return Promise.resolve()
  return loadOnce(rt, 'graph', () =>
    Promise.all([refreshGraph(folderPath), refreshStashCount(folderPath)]).then(() => undefined)
  )
}

/** Load the stash list if it was never loaded; refreshes keep it current afterwards. */
export function loadStashes(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt || getStashes(folderPath)) return Promise.resolve()
  return loadOnce(rt, 'stashes', () => refreshStashes(folderPath))
}

type RepoRefreshListener = (folderPath: string) => Promise<void> | void
const repoListeners = new Set<RepoRefreshListener>()

/** Join every full refresh (branches state subscribes here; it cannot import this module's refresh without a cycle). */
export function onRepoRefresh(listener: RepoRefreshListener): void {
  repoListeners.add(listener)
}

/**
 * Refresh what the folder has loaded. `'summary'` after index-only changes;
 * `'all'` when refs may have moved (graph, stash, branches too).
 */
async function refreshRepoNow(folderPath: string, scope: RefreshScope): Promise<void> {
  if (!gitStore.has(folderPath)) return
  if (scope === 'summary') return refreshGitNow(folderPath)
  const entry = gitStore.get(folderPath)
  await Promise.all([
    refreshGitNow(folderPath),
    entry?.graph ? refreshGraph(folderPath) : undefined,
    entry?.stashes ? refreshStashes(folderPath) : refreshStashCount(folderPath),
    ...[...repoListeners].map((listener) => listener(folderPath))
  ])
}

function refreshRepo(folderPath: string, scope: RefreshScope): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return Promise.resolve()
  const existing = rt.refreshQueue
  if (existing) {
    existing.pendingScope = scope === 'all' || existing.pendingScope === 'all' ? 'all' : 'summary'
    return existing.promise
  }

  const queue: RefreshQueue = { pendingScope: null, promise: Promise.resolve() }
  queue.promise = (async () => {
    let currentScope = scope
    while (runtimes.get(folderPath) === rt) {
      await refreshRepoNow(folderPath, currentScope)
      const nextScope = queue.pendingScope
      if (!nextScope) return
      queue.pendingScope = null
      currentScope = nextScope
    }
  })().finally(() => {
    if (rt.refreshQueue === queue) rt.refreshQueue = null
  })
  rt.refreshQueue = queue
  return queue.promise
}

/** Refresh only working-tree state through the serialized repo queue. */
export function refreshGit(folderPath: string): Promise<void> {
  return refreshRepo(folderPath, 'summary')
}

// MUTATE //

/**
 * Run a git mutation against a folder, then refresh. An `optimistic` patch is
 * applied to the summary before the backend call so the UI moves instantly;
 * the refresh afterwards (also on error) restores ground truth. Errors propagate.
 */
export async function runGitAction<T>(
  folderPath: string,
  fn: (path: string) => Promise<T>,
  options: { scope?: RefreshScope; optimistic?: (summary: GitSummary) => GitSummary } = {}
): Promise<T> {
  const rt = runtimes.get(folderPath)
  const scope = options.scope ?? 'all'
  const current = rt ? getGitSummary(folderPath) : null
  if (options.optimistic && current) gitStore.patch(folderPath, { summary: options.optimistic(current) })
  if (rt) rt.inFlightActions++
  try {
    return await fn(folderPath)
  } finally {
    if (rt && runtimes.get(folderPath) === rt) {
      const remaining = --rt.inFlightActions
      rt.epoch++
      const pendingScope = remaining > 0 ? null : rt.pendingActionScope
      if (pendingScope) rt.pendingActionScope = null
      await refreshRepo(folderPath, scope === 'all' || pendingScope === 'all' ? 'all' : 'summary')
    }
  }
}

export function stageFiles(folderPath: string, files: string[]): Promise<void> {
  return runGitAction(folderPath, (path) => backend.git.stageFiles(path, files), {
    scope: 'summary',
    optimistic: (summary) => stageChanges(summary, files)
  })
}

export function stageAll(folderPath: string): Promise<void> {
  return runGitAction(folderPath, (path) => backend.git.stageAll(path), {
    scope: 'summary',
    optimistic: (summary) => stageChanges(summary, null)
  })
}

export function unstageFiles(folderPath: string, files: string[]): Promise<void> {
  return runGitAction(folderPath, (path) => backend.git.unstageFiles(path, files), {
    scope: 'summary',
    optimistic: (summary) => unstageChanges(summary, files)
  })
}

export function unstageAll(folderPath: string): Promise<void> {
  return runGitAction(folderPath, (path) => backend.git.unstageAll(path), {
    scope: 'summary',
    optimistic: (summary) => unstageChanges(summary, null)
  })
}

export function discardFiles(folderPath: string, files: string[]): Promise<boolean> {
  return withTrashFallback((permanent) => runGitAction(folderPath, (path) => backend.git.discardFiles(path, files, permanent), {
    scope: 'summary',
    optimistic: (summary) => discardChanges(summary, files)
  }))
}

export function discardAll(folderPath: string): Promise<boolean> {
  return withTrashFallback((permanent) => runGitAction(folderPath, (path) => backend.git.discardAll(path, permanent), {
    scope: 'summary',
    optimistic: (summary) => discardChanges(summary, null)
  }))
}

// LIFECYCLE //

let changedListenerReady: Promise<void> | null = null
let focusListenerReady: Promise<void> | null = null

function ensureGitChangedListener(): Promise<void> {
  if (changedListenerReady) return changedListenerReady
  changedListenerReady = backend.git
    .onChanged(({ folder_path, scope, error }) => {
      const rt = runtimes.get(folder_path)
      if (!rt) return
      if (error !== null) {
        if (getGitSummary(folder_path)?.is_repo !== false) {
          gitStore.patch(folder_path, { watchError: error })
        }
        // Only a reported degradation rearms the watcher. Ordinary changes
        // stay on the cheap refresh path.
        void armGitWatch(folder_path)
      }
      if (rt.inFlightActions > 0) {
        rt.pendingActionScope = scope === 'all' || rt.pendingActionScope === 'all' ? 'all' : 'summary'
        return
      }
      void refreshRepo(folder_path, scope)
    })
    .then(() => undefined)
    .catch((error) => {
      changedListenerReady = null
      throw error
    })
  return changedListenerReady
}

function ensureFocusListener(): Promise<void> {
  if (focusListenerReady) return focusListenerReady
  focusListenerReady = platform.focus
    .onChanged((focused) => {
      if (!focused) return
      for (const [folderPath, rt] of runtimes) {
        if (rt.activeRefs > 0) void reconcileActiveFolder(folderPath, true)
      }
    })
    .then(() => undefined)
    .catch((error) => {
      focusListenerReady = null
      throw error
    })
  return focusListenerReady
}

/** Boots the process-wide freshness listeners; safe to call repeatedly. */
function ensureGitListeners(): Promise<void> {
  return Promise.all([ensureGitChangedListener(), ensureFocusListener()]).then(() => undefined)
}

function armGitWatch(folderPath: string): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return Promise.resolve()

  return loadOnce(rt, 'watch', async () => {
    try {
      // Listener registration must win the startup race with both watch
      // activation and the first status read.
      await ensureGitListeners()
      if (runtimes.get(folderPath) !== rt) return
      await backend.git.watch(folderPath)
      if (runtimes.get(folderPath) === rt) gitStore.patch(folderPath, { watchError: null })
    } catch (error) {
      if (runtimes.get(folderPath) === rt && getGitSummary(folderPath)?.is_repo !== false) {
        gitStore.patch(folderPath, { watchError: getErrorMessage(error) })
      }
      console.warn(`Git watch failed for ${folderPath}:`, error)
    }
  })
}

/**
 * Poll ticks refresh the summary only; a forced pass (window focus) refreshes
 * everything loaded, since refs may have moved while events were missed.
 */
async function reconcileActiveFolder(folderPath: string, forceFinalPass = false): Promise<void> {
  const rt = runtimes.get(folderPath)
  if (!rt) return
  await rt.activeReady
  if (runtimes.get(folderPath) !== rt || rt.activeRefs === 0) return

  const scope: RefreshScope = forceFinalPass ? 'all' : 'summary'
  const existing = rt.reconciliation
  if (existing) {
    if (forceFinalPass) await refreshRepo(folderPath, scope)
    return
  }
  if (!forceFinalPass && rt.refreshQueue) return

  const reconciliation = (async () => {
    const wasRepo = gitStore.get(folderPath)?.summary?.is_repo
    await refreshRepo(folderPath, scope)
    if (runtimes.get(folderPath) !== rt) return
    const state = gitStore.get(folderPath)
    if (
      state?.summary?.is_repo !== false &&
      (state?.watchError || (wasRepo === false && state?.summary?.is_repo === true))
    ) {
      await armGitWatch(folderPath)
    }
  })().finally(() => {
    if (rt.reconciliation === reconciliation) rt.reconciliation = null
  })
  rt.reconciliation = reconciliation
  await reconciliation
}

/** Start or repair the folder watcher, then refresh requested state. */
export async function ensureGitWatch(folderPath: string, scope: RefreshScope = 'summary'): Promise<void> {
  // A completed init/clone must not resurrect state after its last tab closed.
  const rt = runtimes.get(folderPath)
  if (!rt) return
  await armGitWatch(folderPath)
  if (runtimes.get(folderPath) !== rt) return
  await refreshRepo(folderPath, scope)
}

/** Reconcile only while this folder is selected in the workbench. */
export function activateGitFolder(folderPath: string): () => void {
  ensureEntry(folderPath)
  const rt = runtimes.get(folderPath)!
  const refs = rt.activeRefs++
  gitStore.startPolling(folderPath, {
    intervalMs: RECONCILE_INTERVAL_MS,
    tick: (path) => reconcileActiveFolder(path)
  })
  if (refs === 0) {
    const ready: Promise<void> = Promise.resolve().then(() =>
      runtimes.get(folderPath) === rt && rt.activeReady === ready ? ensureGitWatch(folderPath) : undefined
    )
    rt.activeReady = ready
  }

  return () => {
    if (runtimes.get(folderPath) !== rt) return
    gitStore.stopPolling(folderPath)
    if (--rt.activeRefs === 0) rt.activeReady = null
  }
}

/** Forget the folder's git state; called when the workbench releases the folder. */
export function releaseGitFolder(folderPath: string): void {
  gitStore.delete(folderPath)
  runtimes.delete(folderPath)
}
