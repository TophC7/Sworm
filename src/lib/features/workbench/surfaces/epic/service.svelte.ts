import type { EpicTab, TabId } from '$lib/features/workbench/model'
import { addEpicTab } from '$lib/features/workbench/state.svelte'

/** Open or focus an epic tab in its owning folder. */
export function openEpicTab(
  folderPath: string,
  epicId: string,
  title: string,
  options: { temporary?: boolean } = {}
): TabId {
  return addEpicTab(folderPath, epicId, title, options.temporary ?? true)
}

export function getEpicTabTitle(tab: EpicTab): string {
  return `${tab.epicId}: ${tab.title}`
}
