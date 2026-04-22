import type { Tab } from '$lib/features/workbench/model'

export type SurfaceKind = 'launcher' | 'session' | 'text' | 'diff' | 'tool'

export function getSurfaceKind(tab: Tab): SurfaceKind {
  return tab.kind
}

export function isSurfacePreview(tab: Tab): boolean {
  return tab.kind !== 'session' && tab.temporary
}
