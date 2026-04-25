import type * as Monaco from 'monaco-editor'
import { StandaloneServices } from 'monaco-editor/esm/vs/editor/standalone/browser/standaloneServices.js'
import { IKeybindingService } from 'monaco-editor/esm/vs/platform/keybinding/common/keybinding.js'
import {
  getCommandsWithKeybindingOverrides,
  getUserKeybindings,
  onOverridesChange
} from '$lib/features/command-palette/shortcuts/overrides.svelte'
import { parseShortcut, type ShortcutStroke } from '$lib/features/command-palette/shortcuts/spec'

type KeybindingService = {
  lookupKeybinding?: (commandId: string) => { getLabel?: () => string | null } | undefined
  getKeybindings?: () => Array<{
    command: string | null
    isDefault?: boolean
    resolvedKeybinding?: { getLabel?: () => string | null }
  }>
}

let monacoRef: typeof Monaco | null = null
let disposable: Monaco.IDisposable | null = null
let unsubscribe: (() => void) | null = null

function service(): KeybindingService | null {
  try {
    return StandaloneServices.get(IKeybindingService) as KeybindingService
  } catch {
    return null
  }
}

function normalizeMonacoLabel(label: string | null | undefined): string | null {
  if (!label) return null
  return label.replace(/\bCtrlCmd\b/g, 'Ctrl').trim() || null
}

export function getMonacoDefaultKeybindings(commandId: string): string[] {
  const keybindingService = service()
  const items = keybindingService?.getKeybindings?.() ?? []
  const labels = new Set<string>()
  for (const item of items) {
    if (item.command !== commandId || item.isDefault !== true) continue
    const label = normalizeMonacoLabel(item.resolvedKeybinding?.getLabel?.())
    if (label) labels.add(label)
  }
  return Array.from(labels)
}

export function getMonacoPrimaryKeybinding(commandId: string): string | null {
  const label = service()?.lookupKeybinding?.(commandId)?.getLabel?.()
  return normalizeMonacoLabel(label)
}

function keyCodeFor(monaco: typeof Monaco, key: string): number | null {
  const { KeyCode } = monaco
  if (/^[a-z]$/i.test(key)) return KeyCode[`Key${key.toUpperCase()}` as keyof typeof KeyCode] as number
  if (/^\d$/.test(key)) return KeyCode[`Digit${key}` as keyof typeof KeyCode] as number
  if (/^f\d+$/i.test(key)) return KeyCode[key.toUpperCase() as keyof typeof KeyCode] as number
  const lower = key.toLowerCase()
  const map: Record<string, number> = {
    '=': KeyCode.Equal,
    '+': KeyCode.Equal,
    '-': KeyCode.Minus,
    ',': KeyCode.Comma,
    '.': KeyCode.Period,
    '/': KeyCode.Slash,
    ';': KeyCode.Semicolon,
    "'": KeyCode.Quote,
    '`': KeyCode.Backquote,
    '[': KeyCode.BracketLeft,
    ']': KeyCode.BracketRight,
    '\\': KeyCode.Backslash,
    enter: KeyCode.Enter,
    escape: KeyCode.Escape,
    esc: KeyCode.Escape,
    tab: KeyCode.Tab,
    space: KeyCode.Space,
    backspace: KeyCode.Backspace,
    delete: KeyCode.Delete,
    home: KeyCode.Home,
    end: KeyCode.End,
    pageup: KeyCode.PageUp,
    pagedown: KeyCode.PageDown,
    up: KeyCode.UpArrow,
    down: KeyCode.DownArrow,
    left: KeyCode.LeftArrow,
    right: KeyCode.RightArrow
  }
  return map[lower] ?? null
}

function encodeStroke(monaco: typeof Monaco, stroke: ShortcutStroke): number | null {
  const keyCode = keyCodeFor(monaco, stroke.key)
  if (keyCode === null || keyCode === undefined) return null
  let encoded = keyCode
  if (stroke.ctrl) encoded |= monaco.KeyMod.CtrlCmd
  if (stroke.shift) encoded |= monaco.KeyMod.Shift
  if (stroke.alt) encoded |= monaco.KeyMod.Alt
  return encoded
}

function encodeShortcut(monaco: typeof Monaco, spec: string): number | null {
  const strokes = parseShortcut(spec)
  if (strokes.length === 0 || strokes.length > 2) return null
  const first = encodeStroke(monaco, strokes[0])
  if (first === null) return null
  if (strokes.length === 1) return first
  const second = encodeStroke(monaco, strokes[1])
  if (second === null) return null
  return monaco.KeyMod.chord(first, second)
}

function applyMonacoUserKeybindings(): void {
  if (!monacoRef) return
  disposable?.dispose()
  disposable = null

  const rules: Monaco.editor.IKeybindingRule[] = []
  for (const command of getCommandsWithKeybindingOverrides()) {
    if (!command.startsWith('editor:')) continue
    const monacoCommand = command.slice('editor:'.length)
    const userBindings = getUserKeybindings(command)
    if (userBindings === null) continue

    for (const defaultBinding of getMonacoDefaultKeybindings(monacoCommand)) {
      const keybinding = encodeShortcut(monacoRef, defaultBinding)
      if (keybinding !== null) rules.push({ keybinding, command: `-${monacoCommand}` })
    }

    for (const binding of userBindings) {
      const keybinding = encodeShortcut(monacoRef, binding)
      if (keybinding !== null) rules.push({ keybinding, command: monacoCommand })
    }
  }

  if (rules.length > 0) disposable = monacoRef.editor.addKeybindingRules(rules)
}

export function attachMonacoKeybindingOverrides(monaco: typeof Monaco): void {
  monacoRef = monaco
  applyMonacoUserKeybindings()
  unsubscribe ??= onOverridesChange(applyMonacoUserKeybindings)
}
