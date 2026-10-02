// Tab insertion heuristics.
//
// New tabs generally open directly next to the active tab so that tabs
// belonging to the same folder stay grouped together naturally.

import { tabServer, type Tab, type TabId } from './model'

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

/** Keep remote server blocks contiguous even when folder-level insertion prefers another slot. */
export function clampTabInsertionToGroup(tabs: Tab[], tab: Tab, target: number): number {
  const server = tabServer(tab)
  const same = server ? tabs.findLastIndex((candidate) => tabServer(candidate) === server) : -1
  if (same >= 0) {
    const first = tabs.findIndex((candidate) => tabServer(candidate) === server)
    return Math.max(first, Math.min(target, same + 1))
  }
  const before = tabs[target - 1] && tabServer(tabs[target - 1])
  const after = tabs[target] && tabServer(tabs[target])
  if (before && before === after) {
    const end = tabs.findLastIndex((candidate) => tabServer(candidate) === before)
    return end + 1
  }
  return target
}

export function reorderWithinGroup(tabs: Tab[], from: number, to: number): Tab[] | null {
  if (from === to) return null
  const tab = tabs[from]
  const server = tabServer(tab)
  const start = server ? tabs.findIndex((item) => tabServer(item) === server) : -1
  const end = server ? tabs.findLastIndex((item) => tabServer(item) === server) : -1
  const next = [...tabs]
  next.splice(from, 1)
  const index = server ? Math.max(start, Math.min(to, end)) : clampTabInsertionToGroup(next, tab, to)
  if (index === from) return null
  next.splice(index, 0, tab)
  return next
}

export function canReorderAt(tabs: Tab[], from: number, slot: number): boolean {
  if (slot === from || slot === from + 1) return false
  const server = tabServer(tabs[from])
  const left = slot > 0 ? tabServer(tabs[slot - 1]) : null
  const right = slot < tabs.length ? tabServer(tabs[slot]) : null
  return server === null ? left === null || right === null || left !== right : left === server || right === server
}

/** A whole group lands only between groups: never inside one, never at its own edges (`server` unknown for another window's group). */
export function canMoveGroupAt(tabs: Tab[], slot: number, server?: string): boolean {
  const left = slot > 0 ? tabServer(tabs[slot - 1]) : null
  const right = slot < tabs.length ? tabServer(tabs[slot]) : null
  return left !== server && right !== server && (left === null || right === null || left !== right)
}

/** Pull each server's tabs into one contiguous block. */
export function compactGroupTabs(tabs: Tab[]): Tab[] {
  const blocks: (Tab | Tab[])[] = []
  const buckets = new Map<string, Tab[]>()
  let previous: string | null = null
  let fragmented = false
  for (const tab of tabs) {
    const server = tabServer(tab)
    const bucket = server ? buckets.get(server) : undefined
    if (!server) blocks.push(tab)
    else if (bucket) {
      fragmented ||= server !== previous
      bucket.push(tab)
    } else {
      const created = [tab]
      buckets.set(server, created)
      blocks.push(created)
    }
    previous = server
  }
  return fragmented ? blocks.flat() : tabs
}
