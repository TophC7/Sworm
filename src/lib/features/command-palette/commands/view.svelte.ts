import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import { getActiveProjectId, hasClosedTabs } from '$lib/features/workbench/state.svelte'
import type { CommandGroup } from './types'
import { closeActiveTab, reloadView, reopenTab } from './actions.svelte'

import {
  isIndentRainbowEnabled,
  toggleIndentRainbow
} from '$lib/features/editor/renderers/monaco/text/indentRainbow.svelte'
import {
  PaintbrushIcon,
  PanelLeftIcon,
  RefreshCwIcon,
  RotateCcwIcon,
  Undo2Icon,
  XIcon,
  ZoomInIcon,
  ZoomOutIcon
} from '$lib/icons/lucideExports'

export function getViewCommands(): CommandGroup[] {
  const activeId = getActiveProjectId()
  const collapsed = isSidebarCollapsed()

  return [
    {
      heading: 'View',
      commands: [
        ...(activeId
          ? [
              {
                id: 'toggle-sidebar',
                label: `${collapsed ? 'Show' : 'Hide'} Sidebar`,
                icon: PanelLeftIcon,
                keywords: ['sidebar', 'panel', 'show', 'hide'],
                onSelect: toggleSidebar
              },
              {
                id: 'close-tab',
                label: 'Close Tab',
                icon: XIcon,
                keywords: ['close', 'tab', 'dismiss'],
                shortcut: 'Ctrl+W',
                onSelect: () => void closeActiveTab()
              },
              ...(hasClosedTabs(activeId)
                ? [
                    {
                      id: 'reopen-closed-tab',
                      label: 'Reopen Closed Tab',
                      icon: Undo2Icon,
                      keywords: ['reopen', 'undo', 'restore', 'tab'],
                      shortcut: 'Ctrl+Shift+T',
                      onSelect: reopenTab
                    }
                  ]
                : [])
            ]
          : []),
        {
          id: 'toggle-indent-rainbow',
          label: `${isIndentRainbowEnabled() ? 'Disable' : 'Enable'} Indent Rainbow`,
          icon: PaintbrushIcon,
          keywords: ['indent', 'rainbow', 'color', 'whitespace', 'toggle'],
          onSelect: toggleIndentRainbow
        },
        {
          id: 'reload-view',
          label: 'Reload View',
          icon: RefreshCwIcon,
          keywords: ['reload', 'refresh', 'hard', 'browser', 'force'],
          shortcut: 'Ctrl+Shift+R',
          onSelect: () => void reloadView()
        },
        {
          id: 'zoom-in',
          label: 'Zoom In',
          icon: ZoomInIcon,
          keywords: ['zoom', 'larger', 'bigger', 'magnify'],
          shortcut: 'Ctrl+=',
          onSelect: zoomIn
        },
        {
          id: 'zoom-out',
          label: 'Zoom Out',
          icon: ZoomOutIcon,
          keywords: ['zoom', 'smaller', 'shrink'],
          shortcut: 'Ctrl+-',
          onSelect: zoomOut
        },
        {
          id: 'zoom-reset',
          label: 'Reset Zoom',
          icon: RotateCcwIcon,
          keywords: ['zoom', 'reset', 'default', '100%'],
          shortcut: 'Ctrl+0',
          onSelect: zoomReset
        }
      ]
    }
  ]
}
