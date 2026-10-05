import { backend } from '$lib/api/backend'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'
import { invalidateLspServerEntries, restartLspServerDefinition } from '$lib/features/editor/lsp/registry'
import { loadLspServers } from './lspSettings.svelte'
import type {
  FormattingSettings,
  LspServerConfig,
  NixSettings,
  ProviderConfig,
  SettingsPayload,
  WindowSettings
} from '$lib/types/backend'

const REMOTE_PREFIX = 'sworm://'

let settings = $state<SettingsPayload | null>(null)
/**
 * Remote folder whose daemon owns the host sections in `settings`, or null
 * when they came from this desktop. Reads, writes and the dialog caption all
 * follow it, so what is on screen and the file a save lands in agree even
 * while the active folder moves under an in-flight request.
 */
let settingsHost = $state<string | null>(null)
let inFlightLoad: { host: string | null; promise: Promise<SettingsPayload> } | null = null
let loadGeneration = 0

export function getSettings() {
  return settings
}

async function fetchSettings(host: string | null): Promise<SettingsPayload> {
  if (!host) return backend.settings.get()

  // Both hosts are read at the layer their setters write — the host's global
  // layer, never folder-merged — so a folder override can't ride a save into
  // the global file. Folder-effective settings stay for execution and
  // diagnostics. Window and terminal describe this window, not the daemon's.
  const [remote, local] = await Promise.all([backend.settings.get(host), backend.settings.get()])
  return { ...remote, window: local.window, terminal: local.terminal }
}

export async function loadSettings(folderPath: string | null = getActiveFolderPath()): Promise<SettingsPayload> {
  // A remote folder's daemon owns its host sections; anything else is this
  // desktop's. Both are read as that host's global layer, the one the setters
  // write.
  const host = folderPath?.startsWith(REMOTE_PREFIX) ? folderPath : null
  if (inFlightLoad?.host === host) return inFlightLoad.promise

  // The newest request owns the store: a slow response for a host the user
  // has since left must never replace the host they are editing now.
  const request = ++loadGeneration
  const promise = fetchSettings(host)
  inFlightLoad = { host, promise }
  try {
    const next = await promise
    if (request === loadGeneration) {
      settings = next
      settingsHost = host
    }
    return next
  } finally {
    if (inFlightLoad?.promise === promise) inFlightLoad = null
  }
}

export async function saveWindowSettings(nextSettings: WindowSettings) {
  const saved = await backend.settings.setWindow(nextSettings)
  if (settings) {
    settings = {
      ...settings,
      window: saved
    }
  }
  return saved
}

async function saveHostSection<T>(
  host: string | null,
  write: (folder?: string) => Promise<T>,
  merge: (current: SettingsPayload, saved: T) => SettingsPayload
): Promise<T> {
  const saved = await write(host ?? undefined)
  if (settings && settingsHost === host) settings = merge(settings, saved)
  return saved
}

export async function saveNixSettings(nextSettings: NixSettings, host: string | null) {
  return saveHostSection(host, (folder) => backend.settings.setNix(nextSettings, folder), (current, saved) => ({
    ...current,
    nix: saved
  }))
}

export async function saveFormattingSettings(nextSettings: FormattingSettings, host: string | null) {
  return saveHostSection(host, (folder) => backend.settings.setFormatting(nextSettings, folder), (current, saved) => ({
    ...current,
    formatting: saved
  }))
}

export async function saveProviderConfig(nextConfig: ProviderConfig, host: string | null) {
  return saveHostSection(host, (folder) => backend.settings.setProviderConfig(nextConfig, folder), (current, saved) => ({
    ...current,
    providers: current.providers.map((entry) =>
      entry.provider.id === saved.provider_id ? { ...entry, config: saved } : entry
    )
  }))
}

export async function saveLspServerConfig(
  nextConfig: LspServerConfig,
  host: string | null
): Promise<LspServerConfig> {
  const saved = await saveHostSection(
    host,
    (folder) => backend.lsp.setServerConfig(nextConfig, folder),
    (current, saved) => {
      const { server_definition_id, ...config } = saved
      return {
        ...current,
        lsp: { ...current.lsp, servers: { ...current.lsp.servers, [server_definition_id]: config } }
      }
    }
  )
  invalidateLspServerEntries()
  await restartLspServerDefinition(saved.server_definition_id)
  await loadLspServers(getActiveFolderPath() ?? undefined)
  return saved
}

/**
 * Remote folder whose daemon owns the loaded host sections, or null for this
 * desktop. Host-section editors key their drafts on it so a workspace switch
 * re-seeds them from the new owner's values instead of writing the old ones.
 */
export function getSettingsHost(): string | null {
  return settingsHost
}

/**
 * Server segment of the loaded settings host's `sworm://<server>/…` URI, or
 * null when it is this desktop. Host-section editors caption it so the user
 * knows which machine the values they see — and write — belong to.
 */
export function getSettingsServer(): string | null {
  if (!settingsHost) return null
  const [server] = settingsHost.slice(REMOTE_PREFIX.length).split('/')
  return server || null
}
