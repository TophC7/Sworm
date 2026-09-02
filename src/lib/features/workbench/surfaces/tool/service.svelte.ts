import type { ToolTab } from '$lib/features/workbench/model'

export function getToolTabTitle(tab: ToolTab): string {
  return tab.label
}
