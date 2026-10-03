// App-menu model for the titlebar hamburger (TitleBarMenu). Reads
// reactive getters; call it from a `$derived` so the structure stays live.

import { platform } from '$lib/platform'
import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import {
  closeCurrentWorkbench,
  newWindow,
  openActiveFolderInExternalTerminal,
  openFolderSettingsFile,
  openSettings,
  reopenTab,
  revealActiveFolderInFileManager
} from '$lib/features/app-actions/actions.svelte'
import { getActiveFolderPath, getActiveTabId, hasClosedTabs } from '$lib/features/workbench/state.svelte'
import { splitRemotePath } from '$lib/utils/paths'

export interface MenuItem {
  kind: 'item'
  label: string
  onSelect: () => void
  shortcut?: string
  disabled?: boolean
}

export interface MenuSeparator {
  kind: 'separator'
}

export type MenuEntry = MenuItem | MenuSeparator

export function buildAppMenu(): MenuEntry[] {
  const hasActive = getActiveTabId() !== null
  const native = platform.capabilities

  return [
    ...(native.nativeWindowControls
      ? [{ kind: 'item' as const, label: 'New Window', shortcut: 'Ctrl+Shift+N', onSelect: () => void newWindow() }]
      : []),
    { kind: 'item', label: 'Reopen Closed Tab', disabled: !hasClosedTabs(), onSelect: reopenTab },
    ...(native.durableWorkbenches
      ? [{ kind: 'item' as const, label: 'Close Workbench', onSelect: () => void closeCurrentWorkbench() }]
      : []),
    { kind: 'separator' },
    ...(native.revealInFileManager
      ? [
          {
            kind: 'item' as const,
            label: 'Reveal Folder in File Manager',
            disabled: !hasActive || Boolean(splitRemotePath(getActiveFolderPath() ?? '')),
            onSelect: revealActiveFolderInFileManager
          }
        ]
      : []),
    ...(native.openInTerminal
      ? [
          {
            kind: 'item' as const,
            label: 'Open Folder in External Terminal',
            disabled: !hasActive,
            onSelect: openActiveFolderInExternalTerminal
          }
        ]
      : []),
    {
      kind: 'item',
      label: 'Folder Settings…',
      disabled: !hasActive,
      onSelect: () => void openFolderSettingsFile()
    },
    { kind: 'item', label: 'Settings…', shortcut: 'Ctrl+,', onSelect: openSettings },
    { kind: 'separator' },
    {
      kind: 'item',
      label: isSidebarCollapsed() ? 'Show Sidebar' : 'Hide Sidebar',
      disabled: !hasActive,
      onSelect: toggleSidebar
    },
    ...(native.zoom
      ? [
          { kind: 'item' as const, label: 'Zoom In', onSelect: zoomIn },
          { kind: 'item' as const, label: 'Zoom Out', onSelect: zoomOut },
          { kind: 'item' as const, label: 'Reset Zoom', onSelect: zoomReset }
        ]
      : [])
  ]
}
