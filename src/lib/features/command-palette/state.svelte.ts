import { closeTransientModals, registerModal } from '$lib/utils/modalRegistry.svelte'

export interface PaletteFilePick {
  folderPath: string
  placeholder: string
  resolve: (path: string | null) => void
}

let commandPaletteOpen = $state(false)
let pendingInitialSearch: string | null = null
let filePick = $state.raw<PaletteFilePick | null>(null)

export function isCommandPaletteOpen(): boolean {
  return commandPaletteOpen
}

export function setCommandPaletteOpen(open: boolean) {
  if (open && !commandPaletteOpen) closeTransientModals()
  commandPaletteOpen = open
  if (!open) settlePaletteFilePick(null)
}

export function toggleCommandPalette() {
  setCommandPaletteOpen(!commandPaletteOpen)
}

export function getPaletteFilePick(): PaletteFilePick | null {
  return filePick
}

/** Opens the palette's file list for one choice; resolves null if it closes without one. */
export function pickFileAsync(folderPath: string, placeholder: string): Promise<string | null> {
  settlePaletteFilePick(null)
  const { promise, resolve } = Promise.withResolvers<string | null>()
  filePick = { folderPath, placeholder, resolve }
  openCommandPaletteWithSearch('')
  return promise
}

export function settlePaletteFilePick(path: string | null): void {
  const pick = filePick
  filePick = null
  pick?.resolve(path)
}

/**
 * Open the palette with a pre-filled search prefix (e.g. `! ` for
 * task mode). The prefix applies exactly once on the next open and
 * is cleared after consumption.
 */
export function openCommandPaletteWithSearch(search: string) {
  pendingInitialSearch = search
  setCommandPaletteOpen(true)
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
