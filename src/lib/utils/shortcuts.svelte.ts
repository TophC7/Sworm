// Global keyboard shortcuts.
//
// Registration uses `registerGlobalBinding` keyed by the same IDs the
// command palette uses — rebinding a command through the palette's
// rebind UI re-dispatches the keyboard listener automatically.
//
// Component-local shortcuts (Monaco's Ctrl+S, Escape inside a context
// menu, etc.) stay in their own components — only shortcuts that work
// regardless of focus context belong here.

import { installKeybindingListener, registerGlobalBinding } from '$lib/utils/keybindings.svelte'
import { toggleCommandPalette, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
import {
  closeActiveTab,
  newEmptyFile,
  newTerminalSession,
  openProjectPicker,
  reloadView,
  reopenTab
} from '$lib/commands/actions.svelte'

/**
 * Register all global keyboard shortcuts. Call once from the root
 * layout's onMount so listeners attach once and clean up on teardown.
 *
 * The binding `id` must match the command id used in the palette so
 * the rebind UI reaches the same entry. Anything without a palette
 * presence (e.g. `toggle-command-palette`) can pick any stable id.
 */
export function setupGlobalShortcuts(): () => void {
  const disposers: Array<() => void> = []

  // Command palette (no palette row, but rebindable via the same store)
  disposers.push(registerGlobalBinding('toggle-command-palette', 'Ctrl+Shift+P', toggleCommandPalette))

  // Zoom
  disposers.push(registerGlobalBinding('zoom-in', 'Ctrl+=', zoomIn))
  disposers.push(registerGlobalBinding('zoom-in-alt', 'Ctrl++', zoomIn))
  disposers.push(registerGlobalBinding('zoom-out', 'Ctrl+-', zoomOut))
  disposers.push(registerGlobalBinding('zoom-reset', 'Ctrl+0', zoomReset))

  // View
  disposers.push(registerGlobalBinding('reload-view', 'Ctrl+Shift+R', () => void reloadView()))

  // Tabs
  disposers.push(registerGlobalBinding('new-file', 'Ctrl+N', newEmptyFile))
  disposers.push(registerGlobalBinding('new-terminal', 'Ctrl+T', () => void newTerminalSession()))
  disposers.push(registerGlobalBinding('close-tab', 'Ctrl+W', () => void closeActiveTab()))
  disposers.push(registerGlobalBinding('reopen-closed-tab', 'Ctrl+Shift+T', reopenTab))

  // Navigation
  disposers.push(registerGlobalBinding('show-project-picker', 'Ctrl+Shift+N', openProjectPicker))

  const uninstall = installKeybindingListener()
  return () => {
    uninstall()
    for (const d of disposers) d()
  }
}
