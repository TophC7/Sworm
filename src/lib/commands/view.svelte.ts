import { isSidebarCollapsed, toggleSidebar, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
import { flushPersistencePending, getActiveProjectId } from '$lib/stores/workspace.svelte'
import { getDirtyEditorsCount, hasAnyDirtyEditors } from '$lib/stores/dirtyEditors.svelte'
import { confirmAsync } from '$lib/stores/confirmService.svelte'
import type { CommandGroup } from './types'

import { isIndentRainbowEnabled, toggleIndentRainbow } from '$lib/editor/extensions/indentRainbow.svelte'
import {
  PaintbrushIcon,
  PanelLeftIcon,
  RefreshCwIcon,
  RotateCcwIcon,
  ZoomInIcon,
  ZoomOutIcon
} from '$lib/icons/lucideExports'

/**
 * Managed reload: confirms unsaved buffers, force-flushes any pending
 * workspace persistence, then triggers the browser reload.
 *
 * Skipping this path (e.g. raw `window.location.reload()`) drops the
 * debounce window of workspace state and the user's unsaved buffers,
 * which is exactly the surprise the recovery spec calls out.
 */
async function reloadView() {
  if (hasAnyDirtyEditors()) {
    const count = getDirtyEditorsCount()
    const noun = count === 1 ? 'file' : 'files'
    const proceed = await confirmAsync({
      title: 'Unsaved changes',
      message: `You have ${count} unsaved ${noun}. Reload and lose changes?`,
      confirmLabel: 'Reload',
      cancelLabel: 'Keep editing'
    })
    if (!proceed) return
  }
  try {
    await flushPersistencePending()
  } catch (error) {
    console.warn('Reload flush failed:', error)
  }
  window.location.reload()
}

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
