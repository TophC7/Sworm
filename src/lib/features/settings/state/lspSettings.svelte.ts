import { backend } from '$lib/api/backend'
import { invalidateLspServerEntries, restartLspServerDefinition } from '$lib/features/editor/lsp/registry'
import type { LspServerConfig, LspServerSettingsEntry } from '$lib/types/backend'

let lspServers = $state<LspServerSettingsEntry[]>([])
let loading = $state(false)
let lastFolderPath = $state<string | undefined>()

export function getLspServers() {
  return lspServers
}

export function getLspServersLoading() {
  return loading
}

export async function loadLspServers(folderPath?: string) {
  lastFolderPath = folderPath
  loading = true
  try {
    lspServers = await backend.lsp.listServers(folderPath)
  } finally {
    loading = false
  }
}

export async function refreshLspServers(folderPath?: string) {
  return loadLspServers(folderPath ?? lastFolderPath)
}

export async function saveLspServerConfig(nextConfig: LspServerConfig, folderPath?: string) {
  const saved = await backend.lsp.setServerConfig(nextConfig)
  // Server config is global, so every folder's cached entries go stale.
  invalidateLspServerEntries()
  await restartLspServerDefinition(saved.server_definition_id)
  await loadLspServers(folderPath ?? lastFolderPath)
  return saved
}
