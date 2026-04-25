import { backend } from '$lib/api/backend'
import { normalizeShortcut, shortcutEquals } from '$lib/features/command-palette/shortcuts/spec'

const APP_STATE_KEY = 'shortcutOverrides'

export interface KeybindingRule {
  command: string
  key: string
}

export interface KeybindingSettingsV1 {
  version: 1
  bindings: KeybindingRule[]
  unboundCommands?: string[]
}

interface ShortcutOverrideState {
  bindings: Record<string, string[]>
  unboundCommands: Set<string>
}

function emptyState(): ShortcutOverrideState {
  return { bindings: {}, unboundCommands: new Set() }
}

function normalizeBindings(bindings: string[]): string[] {
  const seen = new Set<string>()
  const normalized: string[] = []
  for (const binding of bindings) {
    const key = normalizeShortcut(binding)
    if (!key || seen.has(key)) continue
    seen.add(key)
    normalized.push(binding.trim())
  }
  return normalized
}

function parse(raw: string | null): ShortcutOverrideState {
  if (!raw) return emptyState()
  try {
    const parsed = JSON.parse(raw)
    if (parsed?.version !== 1 || !Array.isArray(parsed.bindings)) return emptyState()

    const bindings: Record<string, string[]> = {}
    for (const rule of parsed.bindings as KeybindingRule[]) {
      if (!rule || typeof rule.command !== 'string' || typeof rule.key !== 'string') continue
      bindings[rule.command] = normalizeBindings([...(bindings[rule.command] ?? []), rule.key])
    }

    return {
      bindings,
      unboundCommands: new Set(
        Array.isArray(parsed.unboundCommands)
          ? parsed.unboundCommands.filter((command: unknown): command is string => typeof command === 'string')
          : []
      )
    }
  } catch {
    return emptyState()
  }
}

function serialize(state: ShortcutOverrideState): KeybindingSettingsV1 {
  const rules: KeybindingRule[] = []
  for (const [command, bindings] of Object.entries(state.bindings)) {
    for (const key of bindings) rules.push({ command, key })
  }
  return {
    version: 1,
    bindings: rules,
    unboundCommands: Array.from(state.unboundCommands)
  }
}

let overrides = $state<ShortcutOverrideState>(emptyState())
let loadPromise: Promise<void> | null = null
let changedBeforeLoad = false
const listeners = new Set<() => void>()

function trackOverrides(): void {
  // Reactive tracking for accessors consumed inside $derived.
  overrides.bindings
  overrides.unboundCommands
}

export function loadShortcutOverrides(): Promise<void> {
  loadPromise ??= backend.workspace
    .appStateGet(APP_STATE_KEY)
    .then((raw) => {
      if (changedBeforeLoad) return
      overrides = parse(raw)
      notify()
    })
    .catch((error) => {
      console.warn('Failed to load shortcut overrides:', error)
    })
  return loadPromise
}

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

function persist(state: ShortcutOverrideState): void {
  void backend.workspace.appStatePut(APP_STATE_KEY, JSON.stringify(serialize(state))).catch((error) => {
    console.warn('Failed to persist shortcut overrides:', error)
  })
}

function setState(next: ShortcutOverrideState): void {
  changedBeforeLoad = true
  overrides = next
  persist(next)
  notify()
}

export function getCommandsWithKeybindingOverrides(): string[] {
  trackOverrides()
  return Array.from(new Set([...Object.keys(overrides.bindings), ...Array.from(overrides.unboundCommands)]))
}

export function getUserKeybindings(command: string): string[] | null {
  trackOverrides()
  if (overrides.unboundCommands.has(command)) return []
  const bindings = overrides.bindings[command]
  return bindings ? [...bindings] : null
}

export function getEffectiveBindings(command: string, defaultBindings: string[] | string | undefined): string[] {
  const userBindings = getUserKeybindings(command)
  if (userBindings !== null) return userBindings
  if (Array.isArray(defaultBindings)) return normalizeBindings(defaultBindings)
  if (defaultBindings) return normalizeBindings([defaultBindings])
  return []
}

export function getEffectiveSpec(command: string, defaultSpec: string | undefined): string | undefined {
  return getEffectiveBindings(command, defaultSpec)[0]
}

export function setCommandKeybindings(command: string, bindings: string[]): void {
  const nextBindings = normalizeBindings(bindings)
  const next = {
    bindings: { ...overrides.bindings },
    unboundCommands: new Set(overrides.unboundCommands)
  }
  if (nextBindings.length > 0) {
    next.bindings[command] = nextBindings
    next.unboundCommands.delete(command)
  } else {
    delete next.bindings[command]
    next.unboundCommands.add(command)
  }
  setState(next)
}

export function removeCommandKeybinding(command: string, key: string, defaultBindings: string[] = []): void {
  const next = getEffectiveBindings(command, defaultBindings).filter((binding) => !shortcutEquals(binding, key))
  setCommandKeybindings(command, next)
}

export function clearShortcutOverride(command: string): void {
  const next = {
    bindings: { ...overrides.bindings },
    unboundCommands: new Set(overrides.unboundCommands)
  }
  delete next.bindings[command]
  next.unboundCommands.delete(command)
  setState(next)
}
