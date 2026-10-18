// Folder-keyed branches state using Svelte 5 runes.
//
// Owns the Branches view list, preferences, recents,
// and dirty checkout handling. Preferences persist per workbench and folder
// under the app_state key `branchesView:<id>:<folderPath>`.

import { backend } from '$lib/api/backend'
import { getGitSummary, onRepoRefresh, refreshGit, runGitAction } from '$lib/features/git/state.svelte'
import { isGitError } from '$lib/features/git/errors'
import { confirmAsync, pickAsync, type PickOption } from '$lib/features/confirm/service.svelte'
import { openFolder } from '$lib/features/workbench/state.svelte'
import { splitRemotePath } from '$lib/utils/paths'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import { platform } from '$lib/platform'
import type { BranchSummary, GitSummary, GitWorktree } from '$lib/types/backend'
import { localNameForRemoteRef } from '$lib/features/git/gitRefs'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/utils/client-error'

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

/** Forget the folder's branch state; called when the workbench releases the folder. */
export function releaseBranchFolder(folderPath: string) {
  branchStore.delete(folderPath)
  inFlightLoads.delete(folderPath)
  inFlightFetches.delete(folderPath)
}

// PERSISTENCE //

async function loadPrefs(folderPath: string): Promise<BranchesViewPrefs> {
  const key = `branchesView:${platform.workbench.id}:${folderPath}`
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
    await backend.app.statePut(`branchesView:${platform.workbench.id}:${folderPath}`, JSON.stringify(prefs))
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
  const generation = branchStore.generation(folderPath)
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
    try {
      list = await backend.git.listBranches(folderPath)
    } catch (e) {
      console.error(`Failed to load branches for ${folderPath}:`, e)
    }
    if (branchStore.generation(folderPath) !== generation) return
    branchStore.set(folderPath, {
      list,
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

/** Re-pull `listBranches` and merge into the entry.
 * Prefs and transient flags survive. Runs as part of every full repo
 * refresh (see `onRepoRefresh` below); not called directly by feature code. */
async function refresh(folderPath: string): Promise<void> {
  const generation = branchStore.generation(folderPath)
  try {
    const list = await backend.git.listBranches(folderPath)
    if (branchStore.generation(folderPath) !== generation) return
    branchStore.patch(folderPath, { list })
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
  const generation = branchStore.generation(folderPath)
  const existing = inFlightFetches.get(folderPath)
  if (existing) return existing

  const current = branchStore.get(folderPath)
  if (!current) return Promise.resolve()

  branchStore.patch(folderPath, { fetching: true, fetchedThisSession: true })
  let promise: Promise<void>
  promise = (async () => {
    try {
      await runGitAction(folderPath, (path) => backend.git.remote(path, { kind: 'fetch', remote: null }))
      if (branchStore.generation(folderPath) !== generation) return
      branchStore.patch(folderPath, { fetching: false, fetchedThisSession: true, lastFetchedAt: Date.now() })
    } catch (e) {
      if (branchStore.generation(folderPath) === generation) {
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
 * Occupied checkout is handled by opening (or declining) its existing worktree, never stashing here.
 * Inventory failures skip the offer; Git's own occupancy refusal still routes through handleOccupiedWorktreeError.
 */
export async function offerOccupiedWorktree(folderPath: string, branchName: string): Promise<boolean> {
  let occupied: GitWorktree | undefined
  try {
    const inventory = await backend.git.listWorktrees(folderPath)
    occupied = inventory.worktrees.find(
      (worktree) =>
        worktree.branch === branchName && !worktree.isCurrent && worktree.prunable === null && !worktree.bare
    )
  } catch (e) {
    console.error(`Worktree inventory unavailable for ${folderPath}:`, e)
    return false
  }
  if (!occupied) return false
  const proceed = await confirmAsync({
    title: 'Open Worktree?',
    message: `The branch "${branchName}" is checked out in ${occupied.path}.\n\nOpen that worktree? Changes in this checkout will not be stashed or switched.`,
    confirmLabel: 'Open Worktree'
  })
  if (proceed) await openFolder(occupied.path)
  return true
}

/** A late occupancy race still offers the owning host's checkout and retains any existing stash. */
export async function handleOccupiedWorktreeError(folderPath: string, error: unknown): Promise<boolean> {
  if (!isGitError(error, 'branchInWorktree')) return false
  let occupied: GitWorktree | undefined
  try {
    const inventory = await backend.git.listWorktrees(folderPath)
    occupied = inventory.worktrees.find(
      (worktree) =>
        worktree.branch === error.branch && !worktree.isCurrent && worktree.prunable === null && !worktree.bare
    )
  } catch (e) {
    console.error(`Worktree inventory unavailable for ${folderPath}:`, e)
  }
  const host = splitRemotePath(folderPath)
  const occupiedPath =
    occupied?.path ?? (host && !splitRemotePath(error.path) ? `sworm://${host.server}${error.path}` : error.path)
  const proceed = await confirmAsync({
    title: 'Open Worktree?',
    message: `The branch "${error.branch}" is checked out in ${occupiedPath}.\n\nOpen that worktree? Any recovery stash already created is retained.`,
    confirmLabel: 'Open Worktree'
  })
  if (proceed) await openFolder(occupiedPath)
  return true
}

/** Refresh the summary before deciding whether checkout needs a recovery stash. */
async function dirtyCheck(folderPath: string): Promise<{ dirty: boolean; summary: GitSummary | null }> {
  await refreshGit(folderPath)
  const summary = getGitSummary(folderPath)
  if (!summary || !summary.is_repo) return { dirty: false, summary }
  const dirty = summary.staged_count > 0 || summary.unstaged_count > 0 || summary.untracked_count > 0
  return { dirty, summary }
}

export type CheckoutTarget =
  | { kind: 'branch'; name: string }
  | { kind: 'remote'; remoteName: string; localName: string }
  | { kind: 'detached'; rev: string; label: string }

/** Check out a branch, remote branch (as tracking local) or detached revision; notifies failures. True when HEAD moved. */
export async function checkoutRef(folderPath: string, target: CheckoutTarget): Promise<boolean> {
  try {
    if (target.kind === 'remote') {
      const { localName } = target
      const list = await backend.git.listBranches(folderPath)
      if (list.some((branch) => branch.kind === 'local' && branch.name === localName)) {
        target = { kind: 'branch', name: localName }
      }
    }
    // A remote ref can resolve to the branch already checked out; nothing to switch.
    if (target.kind === 'branch' && getGitSummary(folderPath)?.branch === target.name) return false
    const localName = target.kind === 'branch' ? target.name : target.kind === 'remote' ? target.localName : null
    if (localName !== null && (await offerOccupiedWorktree(folderPath, localName))) return false
    const label = target.kind === 'detached' ? target.label : target.kind === 'branch' ? target.name : target.localName
    const op =
      target.kind === 'branch'
        ? { kind: 'checkout' as const, name: target.name }
        : target.kind === 'remote'
          ? { kind: 'checkout_remote' as const, remoteName: target.remoteName, localName: target.localName }
          : { kind: 'checkout_detached' as const, rev: target.rev }
    let { dirty, summary } = await dirtyCheck(folderPath)
    if (!dirty) {
      try {
        await runGitAction(folderPath, (path) => backend.git.branch(path, op))
        if (localName !== null) markRecent(folderPath, localName)
        return true
      } catch (error) {
        if (!isGitError(error, 'dirtyWorktree')) throw error
        await refreshGit(folderPath)
        summary = getGitSummary(folderPath)
      }
    }
    const staged = summary?.staged_count ?? 0
    const unstaged = summary?.unstaged_count ?? 0
    const untracked = summary?.untracked_count ?? 0
    const moved = await confirmAsync({
      title: `Checkout ${label}?`,
      message: `This working tree has ${staged} staged, ${unstaged} unstaged and ${untracked} untracked changes. Stash them, including untracked files, and switch? They stay recoverable from the Stashes tab.\n\nOther sessions in this folder share this working tree, so switching affects them too.`,
      confirmLabel: 'Stash & Checkout',
      run: () =>
        runGitAction(folderPath, async (path) => {
          await backend.git.stash(path, {
            kind: 'push',
            message: `auto-stash before switch to ${label}`,
            scope: { kind: 'all', includeUntracked: true }
          })
          await backend.git.branch(path, op)
        })
    })
    if (moved && localName !== null) markRecent(folderPath, localName)
    return moved
  } catch (error) {
    if (await handleOccupiedWorktreeError(folderPath, error)) return false
    notify.error('Checkout failed', getErrorMessage(error))
    return false
  }
}

export async function checkoutTo(folderPath: string): Promise<void> {
  await loadFor(folderPath, { autoFetch: false })
  const list = byFolder.get(folderPath)?.list ?? []
  const options: PickOption[] = [
    ...list.filter((branch) => branch.kind === 'local' && !branch.isCurrent),
    ...list.filter((branch) => branch.kind === 'remote')
  ].map((branch) => ({
    value: `${branch.kind === 'local' ? 'branch' : 'remote'}:${branch.name}`,
    label: branch.name,
    description: `${branch.kind === 'local' ? 'Branch' : 'Remote Branch'} · ${branch.tip.shortHash}`
  }))
  try {
    const tags = await backend.git.listTags(folderPath)
    options.push(
      ...tags.map((tag) => ({
        value: `tag:${tag.name}`,
        label: tag.name,
        description: `Tag · ${tag.shortHash}`
      }))
    )
  } catch (error) {
    notify.error('Checkout failed', getErrorMessage(error))
    return
  }
  const value = await pickAsync({
    title: 'Checkout to',
    filterPlaceholder: 'Select a ref to checkout',
    options
  })
  if (value === null) return
  const separator = value.indexOf(':')
  const kind = value.slice(0, separator)
  const name = value.slice(separator + 1)
  const target: CheckoutTarget =
    kind === 'branch'
      ? { kind: 'branch', name }
      : kind === 'remote'
        ? { kind: 'remote', remoteName: name, localName: localNameForRemoteRef(name) }
        : { kind: 'detached', rev: name, label: name }
  await checkoutRef(folderPath, target)
}
