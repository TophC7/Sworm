import { backend } from '$lib/api/backend'
import { onRepoRefresh, refreshRepository } from '$lib/features/git/state.svelte'
import { getTabs } from '$lib/features/workbench/state.svelte'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import type { GitWorktreeList } from '$lib/types/backend'
import { getErrorMessage } from '$lib/utils/client-error'
import { isPathWithin } from '$lib/utils/paths'

interface WorktreeEntry {
  inventory: GitWorktreeList | null
  loading: boolean
  error: string | null
}

const store = createFolderKeyedStore<WorktreeEntry>()
const loadedFolders = new Set<string>()
const inFlight = new Map<string, Promise<GitWorktreeList | null>>()
const requests = new Map<string, number>()

export const byFolder = { get: store.get }

export function releaseWorktreeFolder(folderPath: string): void {
  store.delete(folderPath)
  loadedFolders.delete(folderPath)
  inFlight.delete(folderPath)
  requests.delete(folderPath)
}

export async function loadFor(folderPath: string): Promise<void> {
  if (store.has(folderPath)) {
    await inFlight.get(folderPath)
    return
  }
  await refreshWorktrees(folderPath)
}

/** Latest request wins, including a refresh that overtakes an older inventory read. */
export function refreshWorktrees(folderPath: string): Promise<GitWorktreeList | null> {
  const generation = store.generation(folderPath)
  const request = (requests.get(folderPath) ?? 0) + 1
  requests.set(folderPath, request)
  loadedFolders.add(folderPath)
  store.set(folderPath, { inventory: store.get(folderPath)?.inventory ?? null, loading: true, error: null })
  let pending: Promise<GitWorktreeList | null>
  pending = backend.git
    .listWorktrees(folderPath)
    .then(
      (inventory) => {
        if (store.generation(folderPath) !== generation || requests.get(folderPath) !== request) return null
        store.set(folderPath, { inventory, loading: false, error: null })
        return inventory
      },
      (error: unknown) => {
        if (store.generation(folderPath) === generation && requests.get(folderPath) === request) {
          store.patch(folderPath, { loading: false, error: getErrorMessage(error) })
        }
        return null
      }
    )
    .finally(() => {
      if (inFlight.get(folderPath) === pending) inFlight.delete(folderPath)
    })
  inFlight.set(folderPath, pending)
  return pending
}

onRepoRefresh((folderPath) => (store.has(folderPath) ? refreshWorktrees(folderPath).then(() => {}) : undefined))

/** Refresh sibling views independently of whether Worktrees is currently mounted. */
export async function refreshWorktreeRepository(inventory: GitWorktreeList): Promise<void> {
  const openFolders = new Set(
    getTabs()
      .map((tab) => tab.folderPath)
      .filter((folderPath) => inventory.worktrees.some((worktree) => isPathWithin(worktree.path, folderPath)))
  )
  await Promise.all([
    ...[...openFolders].map((folderPath) => refreshRepository(folderPath)),
    ...[...loadedFolders]
      .filter(
        (folderPath) =>
          !openFolders.has(folderPath) && store.get(folderPath)?.inventory?.commonDir === inventory.commonDir
      )
      .map((folderPath) => refreshWorktrees(folderPath))
  ])
}

export function forgetWorktree(rootPath: string): void {
  for (const folderPath of loadedFolders) {
    if (isPathWithin(rootPath, folderPath)) releaseWorktreeFolder(folderPath)
  }
}
