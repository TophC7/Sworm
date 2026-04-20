import { backend } from '$lib/api/backend'
import type { FormattingSettings, GeneralSettings, ProviderConfig, SettingsPayload } from '$lib/types/backend'

let settings = $state<SettingsPayload | null>(null)
let loading = $state(false)

export function getSettings() {
  return settings
}

export function getSettingsLoading() {
  return loading
}

export async function loadSettings() {
  loading = true
  try {
    settings = await backend.settings.get()
  } finally {
    loading = false
  }
}

export async function saveGeneralSettings(nextSettings: GeneralSettings) {
  const saved = await backend.settings.setGeneral(nextSettings)
  if (settings) {
    settings = {
      ...settings,
      general: saved
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
