import type { CommandGroup } from './types'
import { getActiveProjectId } from '$lib/stores/workspace.svelte'
import { isGitSidebarCollapsed, toggleGitSidebar, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'

import PanelLeftIcon from '@lucide/svelte/icons/panel-left'
import ZoomInIcon from '@lucide/svelte/icons/zoom-in'
import ZoomOutIcon from '@lucide/svelte/icons/zoom-out'
import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw'

export function getViewCommands(): CommandGroup[] {
  const activeId = getActiveProjectId()
  const collapsed = isGitSidebarCollapsed()

  return [
    {
      heading: 'View',
      commands: [
        ...(activeId
          ? [
              {
                id: 'toggle-git-sidebar',
                label: `${collapsed ? 'Show' : 'Hide'} Git Sidebar`,
                icon: PanelLeftIcon,
                keywords: ['sidebar', 'panel', 'git', 'show', 'hide'],
                onSelect: toggleGitSidebar
              }
            ]
          : []),
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
