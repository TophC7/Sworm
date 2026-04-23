import { registerModal } from '$lib/utils/modalRegistry.svelte'

let commandPaletteOpen = $state(false)
let pendingInitialSearch: string | null = null

export function isCommandPaletteOpen(): boolean {
  return commandPaletteOpen
}

export function setCommandPaletteOpen(open: boolean) {
  commandPaletteOpen = open
}

export function toggleCommandPalette() {
  commandPaletteOpen = !commandPaletteOpen
}

/**
 * Open the palette with a pre-filled search prefix (e.g. `! ` for
 * task mode). The prefix applies exactly once on the next open and
 * is cleared after consumption.
 */
export function openCommandPaletteWithSearch(search: string) {
  pendingInitialSearch = search
  commandPaletteOpen = true
}

/** Consume the pending initial search. Returns `null` when none is queued. */
export function consumePendingInitialSearch(): string | null {
  const next = pendingInitialSearch
  pendingInitialSearch = null
  return next
}

registerModal({
  isOpen: isCommandPaletteOpen,
  close: () => setCommandPaletteOpen(false)
})
