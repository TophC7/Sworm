import { backend } from '$lib/api/backend'
import type { LspServerSettingsEntry } from '$lib/types/backend'

let lspServers = $state<LspServerSettingsEntry[]>([])
let loading = $state(false)
let loadGeneration = 0

export function getLspServers() {
  return lspServers
}

export function getLspServersLoading() {
  return loading
}

export async function loadLspServers(folderPath?: string) {
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
