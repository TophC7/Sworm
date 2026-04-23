import type { Tab } from '$lib/features/workbench/model'

export type SurfaceKind = 'launcher' | 'session' | 'text' | 'diff' | 'tool' | 'task'

export function getSurfaceKind(tab: Tab): SurfaceKind {
  return tab.kind
}

export function isSurfacePreview(tab: Tab): boolean {
  if (tab.kind === 'session' || tab.kind === 'task') return false
  return tab.temporary
}
