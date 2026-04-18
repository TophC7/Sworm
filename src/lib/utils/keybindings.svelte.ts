// Global keyboard shortcut registry.
//
// One listener on window. Handlers register with a normalized key string
// ("ctrl+shift+p") and are deduped by that string. Skips xterm focus
// (terminals consume their own keys) but fires against inputs/editors —
// Ctrl/Meta-prefixed shortcuts are never intended as typed input.

import { getEffectiveSpec, onOverridesChange } from '$lib/stores/shortcutOverrides.svelte'

type ShortcutHandler = (event: KeyboardEvent) => void

const bindings = new Map<string, ShortcutHandler>()

// Suspend counter — when non-zero, the global listener no-ops. Lets the
// rebind dialog capture raw keypresses without triggering the real
// command bound to those keys.
let suspendCount = 0

/**
 * Pause global shortcut dispatch until the returned function is called.
 * Safe to nest — dispatch resumes only when every suspend has been
 * released. Used by the rebind dialog during key capture.
 */
export function suspendKeybindings(): () => void {
  suspendCount += 1
  let released = false
  return () => {
    if (released) return
    released = true
    suspendCount = Math.max(0, suspendCount - 1)
  }
}

/**
 * Pretty-print a (possibly lowercase) spec for display.
 * "ctrl+shift+t" → "Ctrl+Shift+T", "ctrl+=" → "Ctrl+=".
 * Leaves Function keys and whole-word keys as title-cased.
 */
export function formatShortcut(spec: string | undefined): string {
  return splitShortcut(spec).join('+')
}

/**
 * Split + prettify a spec into per-key parts.
 * "ctrl+shift+t" → ["Ctrl", "Shift", "T"]. Useful for rendering each
 * key as its own `<Kbd>` chip.
 */
export function splitShortcut(spec: string | undefined): string[] {
  if (!spec) return []
  return splitSpecParts(spec).map((part) => {
    if (part.length === 1) return part.toUpperCase()
    return part[0].toUpperCase() + part.slice(1)
  })
}

// `+` is both the spec separator and a valid key name, so splitting on
// `+` naively drops the final-key token for specs like "Ctrl++".
// Split the trailing key off first using the LAST `+` (ignoring the
// separator role there), then split the modifier prefix normally.
function splitSpecParts(spec: string): string[] {
  const trimmed = spec.trim()
  if (!trimmed) return []
  // A spec without any separator is just the key itself.
  const lastPlus = trimmed.lastIndexOf('+', trimmed.length - 2)
  if (lastPlus < 0) return [trimmed]
  const head = trimmed.slice(0, lastPlus)
  const tail = trimmed.slice(lastPlus + 1)
  const mods = head
    .split('+')
    .map((p) => p.trim())
    .filter(Boolean)
  return [...mods, tail.trim()]
}

/** Normalize a user-facing key spec like "Ctrl+Shift+T" to "ctrl+shift+t". */
function normalizeSpec(spec: string): string {
  const parts = splitSpecParts(spec).map((p) => p.toLowerCase())
  const mods: string[] = []
  let key = ''
  for (const part of parts) {
    if (part === 'ctrl' || part === 'control' || part === 'cmd' || part === 'meta') mods.push('ctrl')
    else if (part === 'shift') mods.push('shift')
    else if (part === 'alt' || part === 'option') mods.push('alt')
    else key = part
  }
  mods.sort()
  return [...mods, key].join('+')
}

/** Normalize an event into the same key format as the specs. */
function normalizeEvent(e: KeyboardEvent): string | null {
  const mods: string[] = []
  if (e.ctrlKey || e.metaKey) mods.push('ctrl')
  if (e.shiftKey) mods.push('shift')
  if (e.altKey) mods.push('alt')

  // Bail on pure-modifier presses
  if (e.key === 'Control' || e.key === 'Meta' || e.key === 'Shift' || e.key === 'Alt') return null

  // Lowercase; "=" stays "=", "T" stays "t"
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key.toLowerCase()

  mods.sort()
  return [...mods, key].join('+')
}

/**
 * Register a global shortcut. Returns a disposer.
 * If the same spec is registered twice, the later one wins and a
 * warning is logged — helps catch accidental collisions early.
 */
export function registerShortcut(spec: string, handler: ShortcutHandler): () => void {
  const key = normalizeSpec(spec)
  if (bindings.has(key)) {
    console.warn(`[keybindings] overwriting existing binding for "${key}"`)
  }
  bindings.set(key, handler)
  return () => {
    if (bindings.get(key) === handler) bindings.delete(key)
  }
}

/**
 * True when the event matches a key the user has explicitly bound to
 * a global shortcut.
 *
 * Intended use: any time we add a new event listener that runs BEFORE
 * the capture-phase global listener (e.g. an even-earlier handler at
 * `document` for a custom widget, a `keypress`-based adapter, or an
 * iframe bridge), call this to decide whether to bail and let the
 * global dispatcher claim the key. Without this guard, such handlers
 * can shadow user-bound shortcuts and waste a debugging afternoon.
 *
 * No current callers — kept exported so we don't have to re-derive the
 * normalization rules the next time a surface needs to ask the
 * question. If nothing uses it after a few months, delete it along
 * with this comment.
 */
export function isBoundShortcut(e: KeyboardEvent): boolean {
  const key = normalizeEvent(e)
  if (!key) return false
  return bindings.has(key)
}

/**
 * Install the single keydown listener. Call once from the root layout.
 *
 * Listens in the CAPTURE phase so user-bound shortcuts win over any
 * focused surface that wants the same key — xterm forwards a Ctrl+V
 * to the PTY, Monaco maps Ctrl+Shift+P to its own palette, INPUTs
 * eat Ctrl+W as delete-prev-word, and so on. Capture phase fires
 * before the event ever reaches the target, so a single
 * preventDefault + stopPropagation here is enough to shadow them
 * all. Unbound keys fall through untouched, leaving normal typing
 * and focused-surface keymaps undisturbed.
 */
export function installKeybindingListener(): () => void {
  function onKeyDown(e: KeyboardEvent) {
    if (suspendCount > 0) return
    const key = normalizeEvent(e)
    if (!key) return
    const handler = bindings.get(key)
    if (!handler) return
    e.preventDefault()
    e.stopPropagation()
    handler(e)
  }
  window.addEventListener('keydown', onKeyDown, { capture: true })
  const unsubscribe = onOverridesChange(reapplyAllGlobalBindings)
  return () => {
    window.removeEventListener('keydown', onKeyDown, { capture: true })
    unsubscribe()
  }
}

// ---------------------------------------------------------------------------
// ID-aware global bindings — user-rebindable via shortcutOverrides store
// ---------------------------------------------------------------------------

interface BindingEntry {
  id: string
  defaultSpec: string | undefined
  handler: ShortcutHandler
  disposer: (() => void) | null
}

const globalBindings = new Map<string, BindingEntry>()

/** Reapply a single binding against the current effective spec. */
function applyBinding(entry: BindingEntry): void {
  entry.disposer?.()
  entry.disposer = null
  const spec = getEffectiveSpec(entry.id, entry.defaultSpec)
  if (!spec) return // explicitly unbound
  entry.disposer = registerShortcut(spec, entry.handler)
}

function reapplyAllGlobalBindings(): void {
  for (const entry of globalBindings.values()) applyBinding(entry)
}

/**
 * Register a rebindable global shortcut. The `id` participates in the
 * user override map — same id used by the command palette row — so
 * settings UI can change the spec and this binding re-attaches
 * automatically. Returns a disposer that removes both the entry and
 * any live keyboard binding.
 */
export function registerGlobalBinding(
  id: string,
  defaultSpec: string | undefined,
  handler: ShortcutHandler
): () => void {
  const existing = globalBindings.get(id)
  if (existing) {
    console.warn(`[keybindings] overwriting global binding for id "${id}"`)
    existing.disposer?.()
  }
  const entry: BindingEntry = { id, defaultSpec, handler, disposer: null }
  globalBindings.set(id, entry)
  applyBinding(entry)
  return () => {
    entry.disposer?.()
    if (globalBindings.get(id) === entry) globalBindings.delete(id)
  }
}
