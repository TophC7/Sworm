// Global keyboard shortcuts.
//
// All app-wide hotkeys are registered here so they live in one place.
// Component-local shortcuts (Escape to close a context menu, Ctrl+S
// inside an editor) stay in their own components — only shortcuts that
// work regardless of focus context belong here.

import { zoomIn, zoomOut, zoomReset, toggleCommandPalette } from '$lib/stores/ui.svelte'

/**
 * Register all global keyboard shortcuts.
 * Call from a top-level $effect in the layout so listeners are
 * attached once and cleaned up on teardown.
 */
export function setupGlobalShortcuts() {
  function onKeyDown(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest?.('.xterm')) return

    if (e.ctrlKey && e.shiftKey && e.key === 'P') {
      e.preventDefault()
      toggleCommandPalette()
      return
    }

    if (!e.ctrlKey && !e.metaKey) return

    switch (e.key) {
      case '=':
      case '+':
        e.preventDefault()
        zoomIn()
        break
      case '-':
        e.preventDefault()
        zoomOut()
        break
      case '0':
        e.preventDefault()
        zoomReset()
        break
    }
  }

  window.addEventListener('keydown', onKeyDown)
  return () => window.removeEventListener('keydown', onKeyDown)
}
