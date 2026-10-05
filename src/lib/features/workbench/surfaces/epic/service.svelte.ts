import type { EpicTab, TabId } from '$lib/features/workbench/model'
import { addEpicTab } from '$lib/features/workbench/state.svelte'

/** Open or focus an epic tab in its owning folder. */
export function openEpicTab(folderPath: string, epicId: string, title: string): TabId {
  return addEpicTab(folderPath, epicId, title)
}

export function getEpicTabTitle(tab: EpicTab): string {
  return `${tab.epicId}: ${tab.title}`
}
