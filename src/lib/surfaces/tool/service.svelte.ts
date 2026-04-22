import { addNotificationToolTab, type ToolTab, type TabId } from '$lib/workbench/state.svelte'

export function openNotificationTool(projectId: string): TabId {
  return addNotificationToolTab(projectId)
}

export function getToolTabTitle(tab: ToolTab): string {
  return tab.label
}
