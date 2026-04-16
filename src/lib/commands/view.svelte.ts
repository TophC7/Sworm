import type { CommandGroup } from './types'
import { getActiveProjectId } from '$lib/stores/workspace.svelte'
import { isSidebarCollapsed, toggleSidebar, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'

import { PanelLeftIcon, ZoomInIcon, ZoomOutIcon, RotateCcwIcon, RefreshCwIcon } from '$lib/icons/lucideExports'

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
              }
            ]
          : []),
        {
          id: 'reload-view',
          label: 'Reload View',
          icon: RefreshCwIcon,
          keywords: ['reload', 'refresh', 'hard', 'browser', 'force'],
          shortcut: 'Ctrl+Shift+R',
          onSelect: () => window.location.reload()
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
