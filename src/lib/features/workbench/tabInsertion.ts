// Tab insertion heuristics.
//
// New tabs generally open directly next to the active tab so that tabs
// belonging to the same folder stay grouped together naturally.

import type { Tab, TabId } from './model'

type TabPlacement = { replaceIndex: number } | { insertIndex: number }

/**
 * Computes where a newly opened tab should be placed in the tab strip.
 *
 * Rules:
 * 1. Launcher tabs for a folder are transient: the first real tab opened
 *    replaces that folder's launcher in-place.
 * 2. Brand-new launcher tabs represent a new folder workspace; append them to the end.
 * 3. If the active tab belongs to the same folder, open directly next to it (activeIndex + 1).
 * 4. If opening into another already-open folder, open after that folder's last tab.
 * 5. Otherwise (no active tab or folder has no tabs), append to the end.
 */
export function computeTabInsertion(tabs: Tab[], activeTabId: TabId | null, tab: Tab): TabPlacement {
  if (tab.kind !== 'launcher') {
    const launcherIndex = tabs.findIndex((t) => t.kind === 'launcher' && t.folderPath === tab.folderPath)
    if (launcherIndex !== -1) {
      return { replaceIndex: launcherIndex }
    }
  }

  if (tab.kind === 'launcher') {
    return { insertIndex: tabs.length }
  }

  const activeIndex = tabs.findIndex((t) => t.id === activeTabId)
  const activeTab = activeIndex !== -1 ? tabs[activeIndex] : null

  if (activeTab && activeTab.folderPath === tab.folderPath) {
    return { insertIndex: activeIndex + 1 }
  }

  const lastIndexForFolder = tabs.findLastIndex((t) => t.folderPath === tab.folderPath)
  if (lastIndexForFolder !== -1) {
    return { insertIndex: lastIndexForFolder + 1 }
  }

  return { insertIndex: tabs.length }
}
