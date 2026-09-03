// App-menu model for the titlebar hamburger (TitleBarMenu). Reads
// reactive getters; call it from a `$derived` so the structure stays live.

import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import {
  openActiveFolderInExternalTerminal,
  openFolderSettingsFile,
  reopenTab,
  revealActiveFolderInFileManager
} from '$lib/features/app-actions/actions.svelte'
import { getActiveTabId, hasClosedTabs } from '$lib/features/workbench/state.svelte'

export interface MenuItem {
  kind: 'item'
  label: string
  onSelect: () => void
  disabled?: boolean
}

export interface MenuSeparator {
  kind: 'separator'
}

export type MenuEntry = MenuItem | MenuSeparator

export function buildAppMenu(): MenuEntry[] {
  const hasActive = getActiveTabId() !== null

  return [
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
  ]
}
