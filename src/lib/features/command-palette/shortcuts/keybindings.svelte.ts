// Global keyboard shortcut registry.
//
// One capture-phase listener on window. Handlers register normalized
// shortcut specs ("ctrl+shift+p", "ctrl+k ctrl+s") and can be rebound
// through the user keybinding settings store.

import { getEffectiveBindings, onOverridesChange } from '$lib/features/command-palette/shortcuts/overrides.svelte'
import {
  formatShortcut,
  normalizeShortcut,
  normalizeShortcutKey,
  splitShortcut
} from '$lib/features/command-palette/shortcuts/spec'
import { logicalKey } from '$lib/utils/keyboardEvent'
import { closeTransientModals } from '$lib/utils/modalRegistry.svelte'

export { formatShortcut, splitShortcut }

type ShortcutHandler = (event: KeyboardEvent) => void

interface RegisteredBinding {
  spec: string
  handler: ShortcutHandler
  skipShell: boolean
  keepsModals: boolean
}

export interface ShortcutOptions {
  skipShell?: boolean
  keepsModals?: boolean
}

const bindings = new Map<string, RegisteredBinding>()

let suspendCount = 0
let pendingChord: { sequence: string; label: string; timer: ReturnType<typeof setTimeout> } | null = null

export function suspendKeybindings(): () => void {
  suspendCount += 1
  let released = false
  return () => {
    if (released) return
    released = true
    suspendCount = Math.max(0, suspendCount - 1)
  }
}

function normalizeEvent(e: KeyboardEvent): string | null {
  // Resolve via `logicalKey` so WebKitGTK's `'Unidentified'` regression
  // on Shift-modified non-character keys (Tab, Enter, …) doesn't drop
  // user-bound shortcuts on the floor.
  const raw = logicalKey(e)
  const mods: string[] = []
  if (e.altKey) mods.push('alt')
  if (e.ctrlKey || e.metaKey) mods.push('ctrl')
  if (e.shiftKey && raw !== '+') mods.push('shift')

  if (raw === 'Control' || raw === 'Meta' || raw === 'Shift' || raw === 'Alt') return null
  const key = normalizeShortcutKey(raw.length === 1 ? raw.toLowerCase() : raw)
  if (!key) return null
  return [...mods, key].join('+')
}

function clearPendingChord(): void {
  if (!pendingChord) return
  clearTimeout(pendingChord.timer)
  pendingChord = null
}

function hasChordPrefix(sequence: string): boolean {
  const prefix = `${sequence} `
  for (const key of bindings.keys()) {
    if (key.startsWith(prefix)) return true
  }
  return false
}

export function registerShortcut(spec: string, handler: ShortcutHandler, options: ShortcutOptions = {}): () => void {
  const key = normalizeShortcut(spec)
  if (!key) return () => {}
  if (bindings.has(key)) {
    console.warn(`[keybindings] overwriting existing binding for "${key}"`)
  }
  const entry: RegisteredBinding = {
    spec: key,
    handler,
    skipShell: options.skipShell ?? false,
    keepsModals: options.keepsModals ?? false
  }
  bindings.set(key, entry)
  return () => {
    if (bindings.get(key) === entry) bindings.delete(key)
    if (pendingChord && (pendingChord.sequence === key || key.startsWith(`${pendingChord.sequence} `))) {
      clearPendingChord()
    }
  }
}

export function isBoundShortcut(e: KeyboardEvent): boolean {
  const stroke = normalizeEvent(e)
  if (!stroke) return false
  if (bindings.has(stroke)) return true
  return hasChordPrefix(stroke)
}

function isTerminalFocused(): boolean {
  const el = document.activeElement as HTMLElement | null
  return !!el?.closest('[data-terminal-focus-scope]')
}

function dispatchBinding(entry: RegisteredBinding, event: KeyboardEvent): void {
  event.preventDefault()
  event.stopPropagation()
  clearPendingChord()
  if (!entry.keepsModals) closeTransientModals()
  entry.handler(event)
}

export function installKeybindingListener(): () => void {
  function onKeyDown(e: KeyboardEvent) {
    if (suspendCount > 0) return
    const stroke = normalizeEvent(e)
    if (!stroke) return

    const sequence = pendingChord ? `${pendingChord.sequence} ${stroke}` : stroke
    const entry = bindings.get(sequence)
    const hasPrefix = hasChordPrefix(sequence)

    if (!entry && !hasPrefix) {
      if (pendingChord) {
        e.preventDefault()
        e.stopPropagation()
        clearPendingChord()
      }
      return
    }

    if (isTerminalFocused() && !(entry?.skipShell ?? false)) {
      if (!pendingChord) return
      e.preventDefault()
      e.stopPropagation()
      clearPendingChord()
      return
    }

    if (entry && !hasPrefix) {
      dispatchBinding(entry, e)
      return
    }

    if (!entry && hasPrefix) {
      e.preventDefault()
      e.stopPropagation()
      clearPendingChord()
      pendingChord = {
        sequence,
        label: sequence,
        timer: setTimeout(clearPendingChord, 5000)
      }
      return
    }

    if (entry) {
      dispatchBinding(entry, e)
    }
  }

  window.addEventListener('keydown', onKeyDown, { capture: true })
  const unsubscribe = onOverridesChange(reapplyAllGlobalBindings)
  return () => {
    clearPendingChord()
    window.removeEventListener('keydown', onKeyDown, { capture: true })
    unsubscribe()
  }
}

interface BindingEntry {
  id: string
  defaultSpecs: string[]
  handler: ShortcutHandler
  skipShell: boolean
  keepsModals: boolean
  disposers: Array<() => void>
}

const globalBindings = new Map<string, BindingEntry>()

function defaultSpecList(defaultSpec: string | string[] | undefined): string[] {
  if (Array.isArray(defaultSpec)) return defaultSpec
  if (defaultSpec) return [defaultSpec]
  return []
}

function applyBinding(entry: BindingEntry): void {
  for (const dispose of entry.disposers) dispose()
  entry.disposers = []
  const specs = getEffectiveBindings(entry.id, entry.defaultSpecs)
  for (const spec of specs) {
    entry.disposers.push(
      registerShortcut(spec, entry.handler, {
        skipShell: entry.skipShell,
        keepsModals: entry.keepsModals
      })
    )
  }
}

function reapplyAllGlobalBindings(): void {
  for (const entry of globalBindings.values()) applyBinding(entry)
}

export function registerGlobalBinding(
  id: string,
  defaultSpec: string | string[] | undefined,
  handler: ShortcutHandler,
  options: ShortcutOptions = {}
): () => void {
  const existing = globalBindings.get(id)
  if (existing) {
    console.warn(`[keybindings] overwriting global binding for id "${id}"`)
    for (const dispose of existing.disposers) dispose()
  }
  const entry: BindingEntry = {
    id,
    defaultSpecs: defaultSpecList(defaultSpec),
    handler,
    skipShell: options.skipShell ?? false,
    keepsModals: options.keepsModals ?? false,
    disposers: []
  }
  globalBindings.set(id, entry)
  applyBinding(entry)
  return () => {
    for (const dispose of entry.disposers) dispose()
    if (globalBindings.get(id) === entry) globalBindings.delete(id)
  }
}
