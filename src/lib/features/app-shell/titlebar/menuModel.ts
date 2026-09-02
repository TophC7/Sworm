// App-menu model for the titlebar hamburger (TitleBarMenu). Reads
// reactive getters; call it from a `$derived` so the structure stays live.

import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import {
  closeActiveTab,
  openActiveFolderInExternalTerminal,
  openFolderPicker,
  openFolderSettingsFile,
  reopenTab,
  revealActiveFolderInFileManager
} from '$lib/features/app-actions/actions.svelte'
import {
  getActiveTabId,
  getRecentUnopenedFolders,
  hasClosedTabs,
  openFolder
} from '$lib/features/workbench/state.svelte'
import { basename } from '$lib/utils/paths'

export interface MenuItem {
  kind: 'item'
  label: string
  onSelect: () => void
  disabled?: boolean
  title?: string
}

export interface MenuSubmenu {
  kind: 'submenu'
  label: string
  items: MenuItem[]
}

export interface MenuSeparator {
  kind: 'separator'
}

export type MenuEntry = MenuItem | MenuSubmenu | MenuSeparator

export function buildAppMenu(): MenuEntry[] {
  const hasActive = getActiveTabId() !== null
  const recent = getRecentUnopenedFolders()

  const entries: MenuEntry[] = [{ kind: 'item', label: 'Open Folder…', onSelect: () => void openFolderPicker() }]
  if (recent.length > 0) {
    entries.push({
      kind: 'submenu',
      label: 'Open Recent',
      items: recent.map((path) => ({
        kind: 'item',
        label: basename(path),
        title: path,
        onSelect: () => void openFolder(path)
      }))
    })
  }
  entries.push(
    { kind: 'separator' },
    { kind: 'item', label: 'Close Tab', disabled: !hasActive, onSelect: () => void closeActiveTab() },
    { kind: 'item', label: 'Reopen Closed Tab', disabled: !hasClosedTabs(), onSelect: reopenTab },
    { kind: 'separator' },
    {
      kind: 'item',
      label: 'Reveal Folder in File Manager',
      disabled: !hasActive,
      onSelect: revealActiveFolderInFileManager
    },
    {
      kind: 'item',
      label: 'Open Folder in External Terminal',
      disabled: !hasActive,
      onSelect: openActiveFolderInExternalTerminal
    },
    { kind: 'item', label: 'Folder Settings…', disabled: !hasActive, onSelect: () => void openFolderSettingsFile() },
    { kind: 'separator' },
    {
      kind: 'item',
      label: isSidebarCollapsed() ? 'Show Sidebar' : 'Hide Sidebar',
      disabled: !hasActive,
      onSelect: toggleSidebar
    },
    { kind: 'item', label: 'Zoom In', onSelect: zoomIn },
    { kind: 'item', label: 'Zoom Out', onSelect: zoomOut },
    { kind: 'item', label: 'Reset Zoom', onSelect: zoomReset }
  )
  return entries
}
