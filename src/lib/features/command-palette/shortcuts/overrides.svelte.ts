// Shortcut override store.
//
// Per-command user-defined keybindings. Layered on top of the default
// specs declared at registration time — a missing entry means "use the
// default", a present entry (even an empty string) means "user set it".
//
// Persisted to localStorage since shortcuts are an app-level preference
// and should survive workspace restore; they have nothing to do with
// project state or the SQLite DB.

const STORAGE_KEY = 'sworm:shortcutOverrides'

function load(): Record<string, string> {
  if (typeof localStorage === 'undefined') return {}
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw)
    if (parsed && typeof parsed === 'object') return parsed as Record<string, string>
  } catch {
    // Corrupt entry — treat as empty so the app still starts.
  }
  return {}
}

function persist(map: Record<string, string>): void {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(map))
  } catch (err) {
    console.warn('Failed to persist shortcut overrides:', err)
  }
}

// Reactive reference so anything reading overrides re-renders when they
// change (e.g. the command palette's shortcut labels after a rebind).
let overrides = $state<Record<string, string>>(load())

// Side-channel for non-reactive consumers (the global keybinding
// registrar) that need to re-bind after an override change without
// being pulled into a Svelte effect context.
const listeners = new Set<() => void>()

export function onOverridesChange(fn: () => void): () => void {
  listeners.add(fn)
  return () => listeners.delete(fn)
}

function notify(): void {
  for (const fn of listeners) {
    try {
      fn()
    } catch (err) {
      console.error('[shortcutOverrides] listener error:', err)
    }
  }
}

export function getOverrides(): Record<string, string> {
  return overrides
}

export function getShortcutOverride(id: string): string | undefined {
  return overrides[id]
}

/**
 * Look up the effective spec for a command id: user override if set,
 * otherwise the caller-provided default. `null` is a valid override
 * meaning "user unbound this command".
 */
export function getEffectiveSpec(id: string, defaultSpec: string | undefined): string | undefined {
  if (id in overrides) return overrides[id] || undefined
  return defaultSpec
}

export function setShortcutOverride(id: string, spec: string): void {
  const next = { ...overrides, [id]: spec }
  overrides = next
  persist(next)
  notify()
}

export function clearShortcutOverride(id: string): void {
  if (!(id in overrides)) return
  const { [id]: _dropped, ...rest } = overrides
  overrides = rest
  persist(rest)
  notify()
}

/** Wipe every override. Useful for a "Restore defaults" settings button. */
export function clearAllShortcutOverrides(): void {
  overrides = {}
  persist({})
  notify()
}
