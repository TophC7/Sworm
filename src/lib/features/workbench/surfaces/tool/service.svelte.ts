import { addNotificationToolTab, type ToolTab, type TabId } from '$lib/features/workbench/state.svelte'

export function openNotificationTool(projectId: string): TabId {
  return addNotificationToolTab(projectId)
}

export function getToolTabTitle(tab: ToolTab): string {
  return tab.label
}
