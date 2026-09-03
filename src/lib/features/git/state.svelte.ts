// Folder-keyed git state module using Svelte 5 runes.
//
// One entry per open folder holds everything the git sidebar and status bar
// read: the working-tree summary, the commit graph, and the stash count/list.
// Freshness comes from three sources, all funnelled through `refreshRepo`:
//   - the backend git-dir watcher (`git-changed`), for external changes;
//   - `runGitAction`, after every in-app mutation;
//   - a refresh on window focus, for the events Linux watchers miss.
// There is deliberately no poll.
//
// Two guards keep concurrent reads from fighting a mutation:
//   - `epochs`: bumped by `runGitAction` when a mutation lands; a summary read
//     that started before the bump is discarded on arrival.
//   - `lastApplied`: a summary read never overwrites one that started later.
// Without them a slow pre-mutation `git status` lands after the post-mutation
// one and the working tree visibly bounces.

import { backend } from '$lib/api/backend'
import { discardChanges, stageChanges, unstageChanges } from '$lib/features/git/git'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import type { GitSummary, GraphCommit, StashEntry } from '$lib/types/backend'
import { getCurrentWindow } from '@tauri-apps/api/window'

const GRAPH_LIMIT = 100

interface RepoState {
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

const epochs = new Map<string, number>()
const lastApplied = new Map<string, number>()
const inFlightActions = new Map<string, number>()
const inFlightLoads = new Map<string, Promise<void>>()
const refreshQueues = new Map<string, RefreshQueue>()
const lifecycles = new Map<string, number>()
let nextSeq = 0
let nextLifecycle = 0

function ensureEntry(folderPath: string) {
  if (!gitStore.has(folderPath)) {
    gitStore.set(folderPath, { summary: null, graph: null, stashCount: null, stashes: null })
    lifecycles.set(folderPath, ++nextLifecycle)
  }
}

function lifecycleFor(folderPath: string): number | null {
  return gitStore.has(folderPath) ? (lifecycles.get(folderPath) ?? null) : null
}

function isCurrentLifecycle(folderPath: string, lifecycle: number | null): boolean {
  return lifecycle !== null && lifecycles.get(folderPath) === lifecycle && gitStore.has(folderPath)
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
export async function refreshGit(folderPath: string): Promise<void> {
  const lifecycle = lifecycleFor(folderPath)
  if (lifecycle === null) return
  const epoch = epochs.get(folderPath) ?? 0
  const seq = ++nextSeq
  try {
    const summary = await backend.git.getSummary(folderPath)
    // A mutation landed meanwhile; its own refresh follows.
    if (!isCurrentLifecycle(folderPath, lifecycle) || (epochs.get(folderPath) ?? 0) !== epoch) return
    // A read that started later already applied.
    if ((lastApplied.get(folderPath) ?? 0) > seq) return
    lastApplied.set(folderPath, seq)
    if (summariesEqual(getGitSummary(folderPath), summary)) return
    gitStore.patch(folderPath, { summary })
  } catch (e) {
    console.error(`Failed to refresh git for ${folderPath}:`, e)
  }
}

async function refreshGraph(folderPath: string): Promise<void> {
  const lifecycle = lifecycleFor(folderPath)
  if (lifecycle === null) return
  try {
    const graph = await backend.git.getGraph(folderPath, GRAPH_LIMIT)
    if (!isCurrentLifecycle(folderPath, lifecycle)) return
    if (graphsEqual(getGitGraph(folderPath), graph)) return
    gitStore.patch(folderPath, { graph })
  } catch (e) {
    console.error(`Failed to load git graph for ${folderPath}:`, e)
  }
}

async function refreshStashCount(folderPath: string): Promise<void> {
  const lifecycle = lifecycleFor(folderPath)
  if (lifecycle === null) return
  try {
    const stashCount = await backend.git.stashCount(folderPath)
    if (!isCurrentLifecycle(folderPath, lifecycle)) return
    gitStore.patch(folderPath, { stashCount })
  } catch (e) {
    console.error(`Failed to count stashes for ${folderPath}:`, e)
  }
}

async function refreshStashes(folderPath: string): Promise<void> {
  const lifecycle = lifecycleFor(folderPath)
  if (lifecycle === null) return
  try {
    const stashes = await backend.git.stashList(folderPath)
    if (!isCurrentLifecycle(folderPath, lifecycle)) return
    gitStore.patch(folderPath, { stashes, stashCount: stashes.length })
  } catch (e) {
    console.error(`Failed to list stashes for ${folderPath}:`, e)
  }
}

/** Coalesce a first load per folder+kind; later callers join the in-flight promise. */
function loadOnce(key: string, load: () => Promise<void>): Promise<void> {
  const existing = inFlightLoads.get(key)
  if (existing) return existing
  const promise = load().finally(() => {
    if (inFlightLoads.get(key) === promise) inFlightLoads.delete(key)
  })
  inFlightLoads.set(key, promise)
  return promise
}

/** Load the graph if it was never loaded; refreshes keep it current afterwards. */
export function loadGraph(folderPath: string): Promise<void> {
  if (getGitGraph(folderPath)) return Promise.resolve()
  return loadOnce(`graph:${folderPath}`, () =>
    Promise.all([refreshGraph(folderPath), refreshStashCount(folderPath)]).then(() => undefined)
  )
}

/** Load the stash list if it was never loaded; refreshes keep it current afterwards. */
export function loadStashes(folderPath: string): Promise<void> {
  if (getStashes(folderPath)) return Promise.resolve()
  return loadOnce(`stashes:${folderPath}`, () => refreshStashes(folderPath))
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
  if (scope === 'summary') return refreshGit(folderPath)
  const entry = gitStore.get(folderPath)
  await Promise.all([
    refreshGit(folderPath),
    entry?.graph ? refreshGraph(folderPath) : undefined,
    entry?.stashes ? refreshStashes(folderPath) : refreshStashCount(folderPath),
    ...[...repoListeners].map((listener) => listener(folderPath))
  ])
}

export function refreshRepo(folderPath: string, scope: RefreshScope): Promise<void> {
  if (!gitStore.has(folderPath)) return Promise.resolve()
  const existing = refreshQueues.get(folderPath)
  if (existing) {
    existing.pendingScope = scope === 'all' || existing.pendingScope === 'all' ? 'all' : 'summary'
    return existing.promise
  }

  const queue: RefreshQueue = { pendingScope: null, promise: Promise.resolve() }
  queue.promise = (async () => {
    let currentScope = scope
    while (true) {
      await refreshRepoNow(folderPath, currentScope)
      const nextScope = queue.pendingScope
      if (!nextScope) return
      queue.pendingScope = null
      currentScope = nextScope
    }
  })().finally(() => {
    if (refreshQueues.get(folderPath) === queue) refreshQueues.delete(folderPath)
  })
  refreshQueues.set(folderPath, queue)
  return queue.promise
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
  const lifecycle = lifecycleFor(folderPath)
  const active = isCurrentLifecycle(folderPath, lifecycle)
  const scope = options.scope ?? 'all'
  const current = active ? getGitSummary(folderPath) : null
  if (options.optimistic && current) gitStore.patch(folderPath, { summary: options.optimistic(current) })
  if (active) inFlightActions.set(folderPath, (inFlightActions.get(folderPath) ?? 0) + 1)
  try {
    return await fn(folderPath)
  } finally {
    if (isCurrentLifecycle(folderPath, lifecycle)) {
      const remaining = (inFlightActions.get(folderPath) ?? 1) - 1
      if (remaining > 0) inFlightActions.set(folderPath, remaining)
      else inFlightActions.delete(folderPath)
      epochs.set(folderPath, (epochs.get(folderPath) ?? 0) + 1)
      await refreshRepo(folderPath, scope)
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

export function discardFiles(folderPath: string, files: string[]): Promise<void> {
  return runGitAction(folderPath, (path) => backend.git.discardFiles(path, files), {
    scope: 'summary',
    optimistic: (summary) => discardChanges(summary, files)
  })
}

export function discardAll(folderPath: string): Promise<void> {
  return runGitAction(folderPath, (path) => backend.git.discardAll(path), {
    scope: 'summary',
    optimistic: (summary) => discardChanges(summary, null)
  })
}

// LIFECYCLE //

let listenersBooted = false

/** Boots the process-wide freshness listeners; safe to call repeatedly. */
export function ensureGitListeners(): void {
  if (listenersBooted) return
  listenersBooted = true

  void backend.git.onChanged(({ folder_path, paths }) => {
    // An in-app action is mid-flight; its own refresh follows and sees the final state.
    if ((inFlightActions.get(folder_path) ?? 0) > 0) return
    const indexOnly = paths.every((path) => path === 'index')
    void refreshRepo(folder_path, indexOnly ? 'summary' : 'all')
  })

  // Safety net for the events Linux watchers miss, mirroring the explorer.
  void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (!focused) return
    for (const folderPath of [...gitStore.keys()]) void refreshRepo(folderPath, 'all')
  })
}

/** Start watching the folder's git dir and refresh it; idempotent. */
export async function ensureGitWatch(folderPath: string, scope: RefreshScope = 'summary'): Promise<void> {
  ensureEntry(folderPath)
  await backend.git.watch(folderPath).catch((e) => console.warn('Git watch failed:', e))
  await refreshRepo(folderPath, scope)
}

/** Forget the folder's git state; called when the workbench releases the folder. */
export function releaseGitFolder(folderPath: string) {
  gitStore.delete(folderPath)
  lifecycles.delete(folderPath)
  epochs.delete(folderPath)
  lastApplied.delete(folderPath)
  inFlightActions.delete(folderPath)
  inFlightLoads.delete(`graph:${folderPath}`)
  inFlightLoads.delete(`stashes:${folderPath}`)
  refreshQueues.delete(folderPath)
}
