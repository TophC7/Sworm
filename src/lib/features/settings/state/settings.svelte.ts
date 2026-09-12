import { backend } from '$lib/api/backend'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'
import type {
  FormattingSettings,
  NixSettings,
  ProviderConfig,
  SettingsPayload,
  TerminalSettings,
  WindowSettings
} from '$lib/types/backend'

const REMOTE_PREFIX = 'sworm://'

let settings = $state<SettingsPayload | null>(null)
let loading = $state(false)
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

export function getSettingsLoading() {
  return loading
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
  loading = true
  try {
    const next = await promise
    if (request === loadGeneration) {
      settings = next
      settingsHost = host
    }
    return next
  } finally {
    if (inFlightLoad?.promise === promise) inFlightLoad = null
    if (request === loadGeneration) loading = false
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

export async function saveTerminalSettings(nextSettings: TerminalSettings) {
  const saved = await backend.settings.setTerminal(nextSettings)
  if (settings) {
    settings = {
      ...settings,
      terminal: saved
    }
  }
  return saved
}

// Host sections live on the machine that owns the values the user just edited,
// so every write below carries `settingsHost` rather than whatever folder is
// active by the time it flushes; window and terminal above stay on this
// desktop. A response is merged only while that host still owns the store.
export async function saveNixSettings(nextSettings: NixSettings) {
  const host = settingsHost
  const saved = await backend.settings.setNix(nextSettings, host ?? undefined)
  if (settings && settingsHost === host) {
    settings = {
      ...settings,
      nix: saved
    }
  }
  return saved
}

export async function saveFormattingSettings(nextSettings: FormattingSettings) {
  const host = settingsHost
  const saved = await backend.settings.setFormatting(nextSettings, host ?? undefined)
  if (settings && settingsHost === host) {
    settings = {
      ...settings,
      formatting: saved
    }
  }
  return saved
}

export async function saveProviderConfig(nextConfig: ProviderConfig) {
  const host = settingsHost
  const saved = await backend.settings.setProviderConfig(nextConfig, host ?? undefined)
  if (settings && settingsHost === host) {
    settings = {
      ...settings,
      providers: settings.providers.map((entry) =>
        entry.provider.id === saved.provider_id ? { ...entry, config: saved } : entry
      )
    }
  }
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
