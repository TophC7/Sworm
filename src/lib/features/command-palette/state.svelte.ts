import { registerModal } from '$lib/utils/modalRegistry.svelte'

let commandPaletteOpen = $state(false)

export function isCommandPaletteOpen(): boolean {
  return commandPaletteOpen
}

export function setCommandPaletteOpen(open: boolean) {
  commandPaletteOpen = open
}

export function toggleCommandPalette() {
  commandPaletteOpen = !commandPaletteOpen
}

registerModal({
  isOpen: isCommandPaletteOpen,
  close: () => setCommandPaletteOpen(false)
})
