import type { EpicTab, TabId } from '$lib/features/workbench/model'
import { addEpicTab, openProject, promoteTab, restoreWorkspaceFromDisk } from '$lib/features/workbench/state.svelte'

/**
 * Open (or focus) an epic surface tab. Mirrors the issue surface
 * service so re-opening the same epic focuses the existing tab
 * instead of stacking duplicates.
 */
export async function openEpicTab(
  projectId: string,
  epicId: string,
  title: string,
  options: { temporary?: boolean } = {}
): Promise<TabId> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
  return addEpicTab(projectId, epicId, title, options.temporary ?? true)
}

export function promoteEpicTab(projectId: string, tabId: TabId): void {
  promoteTab(projectId, tabId)
}

export function getEpicTabTitle(tab: EpicTab): string {
  return `${tab.epicId}: ${tab.title}`
}
