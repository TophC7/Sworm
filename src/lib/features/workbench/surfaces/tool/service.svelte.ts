import type { TabId, ToolTab } from '$lib/features/workbench/model'
import { addNotificationToolTab, openProject, restoreWorkspaceFromDisk } from '$lib/features/workbench/state.svelte'

export async function openNotificationTool(projectId: string): Promise<TabId> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
  return addNotificationToolTab(projectId)
}

export function getToolTabTitle(tab: ToolTab): string {
  return tab.label
}
