import { backend } from '$lib/api/backend'
import { invalidateLspServerEntries, restartLspServerDefinition } from '$lib/lsp/registry'
import type { LspServerConfig, LspServerSettingsEntry } from '$lib/types/backend'

let lspServers = $state<LspServerSettingsEntry[]>([])
let loading = $state(false)
let lastProjectId = $state<string | undefined>()

export function getLspServers() {
  return lspServers
}

export function getLspServersLoading() {
  return loading
}

export async function loadLspServers(projectId?: string) {
  lastProjectId = projectId
  loading = true
  try {
    lspServers = await backend.lsp.listServers(projectId)
  } finally {
    loading = false
  }
}

export async function refreshLspServers(projectId?: string) {
  return loadLspServers(projectId ?? lastProjectId)
}

export async function saveLspServerConfig(nextConfig: LspServerConfig, projectId?: string) {
  const saved = await backend.lsp.setServerConfig(nextConfig)
  invalidateLspServerEntries()
  await restartLspServerDefinition(saved.server_definition_id)
  await loadLspServers(projectId ?? lastProjectId)
  return saved
}
