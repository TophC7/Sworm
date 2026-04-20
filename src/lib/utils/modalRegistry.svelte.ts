// Modal registry.
//
// Central record of modals that hold DOM focus while open. Two jobs:
//
//   1. Focus-restore signal — the keyboard dispatcher and the root
//      layout need to know "is ANY modal currently stealing focus?"
//      so the active terminal can be refocused the moment all modals
//      close. Works for both transient (palette, settings) and
//      blocking (confirm, prompts) modals equally — they all steal
//      focus the same way.
//
//   2. Transient auto-dismiss — when the user fires an action
//      shortcut, any "transient" modals (ones the user opened as a
//      means to an end) dismiss so the action's effect isn't hidden
//      behind a stale surface. Blocking modals (a confirm prompt,
//      a required decision) must NOT auto-dismiss — the user's
//      pending answer cannot be dropped.
//
// A registered modal is "transient" iff it supplies a `close`
// callback. Omit `close` for blocking modals.
//
// Uses `SvelteSet` so that registrations/unregistrations emit
// reactive signals — a `$derived` calling `isAnyModalOpen()` will
// re-evaluate when DialogContent mounts or unmounts.

import { SvelteSet } from 'svelte/reactivity'

interface Modal {
  isOpen: () => boolean
  /**
   * Optional dismiss hook. Provided only by transient modals
   * (palette, settings). `closeTransientModals()` iterates modals
   * and invokes this on any that are open.
   */
  close?: () => void
}

const modals = new SvelteSet<Modal>()

/**
 * Register a modal with the registry. Returns a disposer that removes
 * it — typical use is `$effect(() => registerModal(handle))` so the
 * handle's lifetime matches the modal's DOM lifetime.
 */
export function registerModal(handle: Modal): () => void {
  modals.add(handle)
  return () => {
    modals.delete(handle)
  }
}

/**
 * Close every open TRANSIENT modal. Called by the keybinding
 * dispatcher before firing a binding whose `keepsModals` is false.
 * Blocking modals (no `close` provided) are skipped.
 */
export function closeTransientModals(): void {
  for (const m of modals) {
    if (m.close && m.isOpen()) m.close()
  }
}

/**
 * True when any registered modal — transient or blocking — is
 * currently open. Drives terminal focus-restore: as long as any
 * modal holds focus, we don't refocus the terminal; the instant the
 * last one closes, we reassert focus on the active session.
 */
export function isAnyModalOpen(): boolean {
  for (const m of modals) {
    if (m.isOpen()) return true
  }
  return false
}
