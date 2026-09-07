import { backend } from '$lib/api/backend'
import type {
  FormattingSettings,
  NixSettings,
  ProviderConfig,
  SettingsPayload,
  TerminalSettings,
  WindowSettings
} from '$lib/types/backend'

let settings = $state<SettingsPayload | null>(null)
let loading = $state(false)
let inFlightLoad: Promise<SettingsPayload> | null = null

export function getSettings() {
  return settings
}

export function getSettingsLoading() {
  return loading
}

export async function loadSettings() {
  if (inFlightLoad) return inFlightLoad
  loading = true
  inFlightLoad = backend.settings.get()
  try {
    settings = await inFlightLoad
    return settings
  } finally {
    loading = false
    inFlightLoad = null
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

export async function saveNixSettings(nextSettings: NixSettings) {
  const saved = await backend.settings.setNix(nextSettings)
  if (settings) {
    settings = {
      ...settings,
      nix: saved
    }
  }
  return saved
}

export async function saveFormattingSettings(nextSettings: FormattingSettings) {
  const saved = await backend.settings.setFormatting(nextSettings)
  if (settings) {
    settings = {
      ...settings,
      formatting: saved
    }
  }
  return saved
}

export async function saveProviderConfig(nextConfig: ProviderConfig) {
  const saved = await backend.settings.setProviderConfig(nextConfig)
  if (settings) {
    settings = {
      ...settings,
      providers: settings.providers.map((entry) =>
        entry.provider.id === saved.provider_id ? { ...entry, config: saved } : entry
      )
    }
  }
  return saved
}
