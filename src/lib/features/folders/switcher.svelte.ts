import { closeTransientModals, registerModal } from '$lib/utils/modalRegistry.svelte'

let open = $state(false)

export function isFolderSwitcherOpen(): boolean {
  return open
}

export function setFolderSwitcherOpen(value: boolean): void {
  if (value && !open) closeTransientModals()
  open = value
}

export function toggleFolderSwitcher(): void {
  setFolderSwitcherOpen(!open)
}

registerModal({ isOpen: isFolderSwitcherOpen, close: () => setFolderSwitcherOpen(false) })
