// Provider state module using Svelte 5 runes.
//
// `providers` is the global detection (settings view, fallback). Folder
// detections run inside the folder's Nix environment and are keyed by
// folder so out-of-order responses never clobber another folder.

import { backend } from '$lib/api/backend'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import type { ProviderStatus } from '$lib/types/backend'

let providers = $state<ProviderStatus[]>([])
let loading = $state(false)
const folderProviders = createFolderKeyedStore<{ providers: ProviderStatus[] }>()
const folderGenerations = new Map<string, number>()

export function getProvidersLoading() {
  return loading
}

export async function loadProviders() {
  loading = true
  try {
    providers = await backend.providers.list()
  } catch (e) {
    console.error('Failed to load providers:', e)
  } finally {
    loading = false
  }
}

export async function refreshProviders() {
  loading = true
  try {
    providers = await backend.providers.list()
  } catch (e) {
    console.error('Failed to refresh providers:', e)
    throw e
  } finally {
    loading = false
  }
}

/** Detect providers inside the folder's environment. On failure the entry stays absent and the global list applies. */
export async function loadProvidersForFolder(folderPath: string) {
  const generation = folderGenerations.get(folderPath) ?? 0
  loading = true
  try {
    const nextProviders = await backend.providers.listForFolder(folderPath)
    if ((folderGenerations.get(folderPath) ?? 0) !== generation) return
    folderProviders.set(folderPath, { providers: nextProviders })
  } catch (e) {
    console.warn(`Failed to load providers for ${folderPath}:`, e)
  } finally {
    loading = false
  }
}

/** Connected providers for `folderPath`, falling back to the global detection when the folder has none loaded. */
export function getConnectedProviders(folderPath: string | null): ProviderStatus[] {
  const source = (folderPath ? folderProviders.get(folderPath)?.providers : undefined) ?? providers
  return source.filter((p) => p.status === 'connected')
}

/** Forget the folder's detection; called when the workbench releases the folder. */
export function releaseProviderFolder(folderPath: string) {
  folderGenerations.set(folderPath, (folderGenerations.get(folderPath) ?? 0) + 1)
  folderProviders.delete(folderPath)
}
