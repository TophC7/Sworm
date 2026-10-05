// One capture-phase listener dispatches effective app command bindings.

import {
  getAppCommandDefinitions,
  type AppCommandDefinition
} from '$lib/features/command-palette/commands/registry.svelte'
import {
  getEffectiveBindings,
  loadShortcutOverrides,
  onOverridesChange
} from '$lib/features/command-palette/shortcuts/overrides.svelte'
import { normalizeShortcut, normalizeShortcutKey } from '$lib/features/command-palette/shortcuts/spec'
import { logicalKey } from '$lib/utils/keyboardEvent'
import { closeTransientModals } from '$lib/utils/modalRegistry.svelte'


const bindings = new Map<string, Pick<AppCommandDefinition, 'run' | 'terminalPolicy'>>()

let suspendCount = 0
let pendingChord: { sequence: string; timer: ReturnType<typeof setTimeout> } | null = null

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


function isTerminalFocused(): boolean {
  const el = document.activeElement as HTMLElement | null
  return !!el?.closest('[data-terminal-focus-scope]')
}

function dispatchBinding(entry: Pick<AppCommandDefinition, 'run' | 'terminalPolicy'>, event: KeyboardEvent): void {
  event.preventDefault()
  event.stopPropagation()
  clearPendingChord()
  const keepsModals = entry.terminalPolicy === 'skip-shell-keeps-modals'
  if (!keepsModals) closeTransientModals()
  void entry.run()
}

function installKeybindingListener(): () => void {
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

    const skipShell = !!entry?.terminalPolicy
    if (isTerminalFocused() && !skipShell) {
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
        timer: setTimeout(clearPendingChord, 5000)
      }
      return
    }

    if (entry) {
      dispatchBinding(entry, e)
    }
  }

  window.addEventListener('keydown', onKeyDown, { capture: true })
  return () => {
    clearPendingChord()
    window.removeEventListener('keydown', onKeyDown, { capture: true })
  }
}

function rebuild(): void {
  clearPendingChord()
  bindings.clear()
  for (const { id, defaultKeybindings, run, terminalPolicy } of getAppCommandDefinitions()) {
    for (const spec of getEffectiveBindings(id, defaultKeybindings ?? [])) {
      const key = normalizeShortcut(spec)
      if (key) bindings.set(key, { run, terminalPolicy })
    }
  }
}

export function setupGlobalShortcuts(): () => void {
  void loadShortcutOverrides()
  rebuild()
  const unsubscribe = onOverridesChange(rebuild)
  const uninstall = installKeybindingListener()
  return () => {
    uninstall()
    unsubscribe()
    bindings.clear()
  }
}
