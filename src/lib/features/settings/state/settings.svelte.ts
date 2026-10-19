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
  TypographySettings,
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

type DesktopSettings = Pick<SettingsPayload, 'window' | 'typography'>
/** This desktop's own sections, loaded apart from any remote daemon so fonts and window chrome survive its outage. */
let desktopSettings = $state<DesktopSettings | null>(null)
let desktopGeneration = 0

export function getSettings() {
  return settings
}

export function getDesktopSettings() {
  return desktopSettings
}

async function loadDesktopSettings(): Promise<SettingsPayload> {
  // The newest read wins, as with `loadGeneration`.
  const request = ++desktopGeneration
  const local = await backend.settings.get()
  if (request === desktopGeneration) desktopSettings = { window: local.window, typography: local.typography }
  return local
}

async function fetchSettings(host: string | null): Promise<SettingsPayload> {
  const local = loadDesktopSettings()
  if (!host) return local

  // Host sections come from the daemon's global layer, the one its setters
  // write, so a folder override can't ride a save into the global file.
  // Folder-effective settings stay for execution and diagnostics. Window and
  // typography are read from `desktopSettings`; `local` lands there even when
  // the daemon rejects.
  const [remote] = await Promise.all([backend.settings.get(host), local])
  return remote
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
  if (desktopSettings) desktopSettings = { ...desktopSettings, window: saved }
  return saved
}

export async function saveTypographySettings(nextSettings: TypographySettings) {
  const saved = await backend.settings.setTypography(nextSettings)
  if (desktopSettings) desktopSettings = { ...desktopSettings, typography: saved }
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
  return saveHostSection(
    host,
    (folder) => backend.settings.setNix(nextSettings, folder),
    (current, saved) => ({
      ...current,
      nix: saved
    })
  )
}

export async function saveFormattingSettings(nextSettings: FormattingSettings, host: string | null) {
  return saveHostSection(
    host,
    (folder) => backend.settings.setFormatting(nextSettings, folder),
    (current, saved) => ({
      ...current,
      formatting: saved
    })
  )
}

export async function saveProviderConfig(nextConfig: ProviderConfig, host: string | null) {
  return saveHostSection(
    host,
    (folder) => backend.settings.setProviderConfig(nextConfig, folder),
    (current, saved) => ({
      ...current,
      providers: current.providers.map((entry) =>
        entry.provider.id === saved.provider_id ? { ...entry, config: saved } : entry
      )
    })
  )
}

export async function saveLspServerConfig(nextConfig: LspServerConfig, host: string | null): Promise<LspServerConfig> {
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
