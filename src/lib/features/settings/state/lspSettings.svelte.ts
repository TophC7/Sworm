import { backend } from '$lib/api/backend'
import { invalidateLspServerEntries, restartLspServerDefinition } from '$lib/features/editor/lsp/registry'
import type { LspServerConfig, LspServerSettingsEntry } from '$lib/types/backend'

let lspServers = $state<LspServerSettingsEntry[]>([])
let loading = $state(false)
let lastFolderPath = $state<string | undefined>()
let loadGeneration = 0

export function getLspServers() {
  return lspServers
}

export function getLspServersLoading() {
  return loading
}

export async function loadLspServers(folderPath?: string) {
  lastFolderPath = folderPath
  // The newest request owns the list: a slow response for a folder the user
  // has since left must not replace the active folder's servers.
  const request = ++loadGeneration
  loading = true
  try {
    const entries = await backend.lsp.listServers(folderPath)
    if (request !== loadGeneration) return
    lspServers = entries
  } finally {
    if (request === loadGeneration) loading = false
  }
}

export async function refreshLspServers(folderPath?: string) {
  return loadLspServers(folderPath ?? lastFolderPath)
}

export async function saveLspServerConfig(nextConfig: LspServerConfig, folderPath?: string) {
  // `lsp` is a host section: the folder decides which machine's settings file
  // the write lands in. Either way it is global there, so caches go stale.
  const saved = await backend.lsp.setServerConfig(nextConfig, folderPath ?? lastFolderPath)
  invalidateLspServerEntries()
  await restartLspServerDefinition(saved.server_definition_id)
  await loadLspServers(folderPath ?? lastFolderPath)
  return saved
}
