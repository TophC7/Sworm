// Global keyboard shortcuts.
//
// Registration uses `registerGlobalBinding` keyed by the same IDs the
// command palette uses — rebinding a command through the palette's
// rebind UI re-dispatches the keyboard listener automatically.
//
// Component-local shortcuts (Monaco's Ctrl+S, Escape inside a context
// menu, etc.) stay in their own components — only shortcuts that work
// regardless of focus context belong here.
//
// `skipShell: true` marks escape-hatch bindings that fire even when a
// terminal has DOM focus. Reserve for commands the user must be able
// to reach from inside a TUI — palette, zoom, reload, project picker.
// Everything else defers to the shell when the terminal is focused,
// so readline/tmux/nvim keep their native bindings.

import {
  installKeybindingListener,
  registerGlobalBinding
} from '$lib/features/command-palette/shortcuts/keybindings.svelte'
import { toggleCommandPalette } from '$lib/features/command-palette/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import {
  closeActiveTab,
  newEmptyFile,
  newTerminalSession,
  openProjectPicker,
  reloadView,
  reopenTab
} from '$lib/features/command-palette/commands/actions.svelte'

// Fires even when a terminal has focus.
const ESCAPE_HATCH = { skipShell: true } as const

// Fires even when a terminal has focus AND leaves any open transient
// modal (palette, settings) untouched. Use for modal toggles (which
// would fight auto-close) and in-modal adjustments (zoom) where the
// user is reasonably adjusting chrome while the modal stays up.
const ESCAPE_HATCH_KEEPS_MODALS = { skipShell: true, keepsModals: true } as const

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

  // Command palette — escape hatch from TUIs. `keepsModals` because
  // toggle-palette closes the palette itself; auto-closing it first
  // would make the toggle re-open instead of closing.
  disposers.push(
    registerGlobalBinding('toggle-command-palette', 'Ctrl+Shift+P', toggleCommandPalette, ESCAPE_HATCH_KEEPS_MODALS)
  )

  // Zoom — view chrome. `keepsModals` so a user zooming while the
  // palette or settings dialog is open keeps the surface in view.
  disposers.push(registerGlobalBinding('zoom-in', 'Ctrl+=', zoomIn, ESCAPE_HATCH_KEEPS_MODALS))
  disposers.push(registerGlobalBinding('zoom-in-alt', 'Ctrl++', zoomIn, ESCAPE_HATCH_KEEPS_MODALS))
  disposers.push(registerGlobalBinding('zoom-out', 'Ctrl+-', zoomOut, ESCAPE_HATCH_KEEPS_MODALS))
  disposers.push(registerGlobalBinding('zoom-reset', 'Ctrl+0', zoomReset, ESCAPE_HATCH_KEEPS_MODALS))

  // View — reload wipes the webview, so modal state is moot anyway;
  // default auto-close is fine.
  disposers.push(registerGlobalBinding('reload-view', 'Ctrl+Shift+R', () => void reloadView(), ESCAPE_HATCH))

  // Tabs — yield to the shell when the terminal is focused. Ctrl+W is
  // readline delete-word, Ctrl+T is tmux/transpose, Ctrl+N is history
  // next / nvim window split. Users reach these via the palette or UI
  // when inside a terminal.
  disposers.push(registerGlobalBinding('new-file', 'Ctrl+N', newEmptyFile))
  disposers.push(registerGlobalBinding('new-terminal', 'Ctrl+T', () => void newTerminalSession()))
  disposers.push(registerGlobalBinding('close-tab', 'Ctrl+W', () => void closeActiveTab()))
  disposers.push(registerGlobalBinding('reopen-closed-tab', 'Ctrl+Shift+T', reopenTab))

  // Navigation
  disposers.push(registerGlobalBinding('show-project-picker', 'Ctrl+Shift+N', openProjectPicker, ESCAPE_HATCH))

  const uninstall = installKeybindingListener()
  return () => {
    uninstall()
    for (const d of disposers) d()
  }
}
