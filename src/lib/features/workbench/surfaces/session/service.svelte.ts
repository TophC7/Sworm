import { allProviders, directOptions } from '$lib/features/sessions/providers/catalog'
import type { SessionTab } from '$lib/features/workbench/model'

export function getSessionTabTitle(tab: SessionTab): string {
  return tab.title
}

export function getSessionProviderIcon(tab: SessionTab): string | null {
  const provider =
    allProviders.find((entry) => entry.id === tab.providerId) ??
    directOptions.find((entry) => entry.id === tab.providerId)
  return provider?.icon ?? null
}
