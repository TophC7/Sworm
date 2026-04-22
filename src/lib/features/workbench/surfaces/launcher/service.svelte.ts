import {
  closeLauncherTabInPane,
  openLauncherTab,
  type PaneSlot,
  type TabId
} from '$lib/features/workbench/state.svelte'

export function openLauncherSurface(projectId: string, paneSlot?: PaneSlot): TabId {
  return openLauncherTab(projectId, paneSlot)
}

export function closeLauncherSurface(projectId: string, paneSlot: PaneSlot): void {
  closeLauncherTabInPane(projectId, paneSlot)
}

export function getLauncherTitle(): string {
  return 'New'
}
