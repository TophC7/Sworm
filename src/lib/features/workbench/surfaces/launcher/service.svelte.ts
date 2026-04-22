import type { PaneSlot, TabId } from '$lib/features/workbench/model'
import {
  closeLauncherTabInPane,
  openLauncherTab,
  openProject,
  restoreWorkspaceFromDisk
} from '$lib/features/workbench/state.svelte'

export async function openLauncherSurface(projectId: string, paneSlot?: PaneSlot): Promise<TabId> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
  return openLauncherTab(projectId, paneSlot)
}

export function closeLauncherSurface(projectId: string, paneSlot: PaneSlot): void {
  closeLauncherTabInPane(projectId, paneSlot)
}

export function getLauncherTitle(): string {
  return 'New'
}
