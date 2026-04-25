export interface ShortcutStroke {
  ctrl: boolean
  shift: boolean
  alt: boolean
  key: string
}

export function splitShortcutSequence(spec: string | undefined): string[][] {
  if (!spec) return []
  return spec
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map(splitShortcutParts)
    .filter((parts) => parts.length > 0)
}

export function splitShortcutParts(spec: string): string[] {
  const trimmed = spec.trim()
  if (!trimmed) return []
  const lastPlus = trimmed.lastIndexOf('+', trimmed.length - 2)
  if (lastPlus < 0) return [trimmed]
  const head = trimmed.slice(0, lastPlus)
  const tail = trimmed.slice(lastPlus + 1)
  const mods = head
    .split('+')
    .map((part) => part.trim())
    .filter(Boolean)
  return [...mods, tail.trim()]
}

export function parseShortcutStroke(spec: string): ShortcutStroke | null {
  const parts = splitShortcutParts(spec)
  if (parts.length === 0) return null
  const stroke: ShortcutStroke = { ctrl: false, shift: false, alt: false, key: '' }
  for (const rawPart of parts) {
    const part = rawPart.toLowerCase()
    if (part === 'ctrl' || part === 'control' || part === 'cmd' || part === 'meta') stroke.ctrl = true
    else if (part === 'shift') stroke.shift = true
    else if (part === 'alt' || part === 'option') stroke.alt = true
    else stroke.key = normalizeShortcutKey(rawPart)
  }
  return stroke.key ? stroke : null
}

export function parseShortcut(spec: string): ShortcutStroke[] {
  return spec
    .trim()
    .split(/\s+/)
    .map(parseShortcutStroke)
    .filter((stroke): stroke is ShortcutStroke => stroke !== null)
}

export function normalizeShortcut(spec: string): string {
  return parseShortcut(spec).map(normalizeStroke).join(' ')
}

export function normalizeShortcutKey(key: string): string {
  const trimmed = key.trim()
  if (!trimmed) return ''
  const lower = trimmed.toLowerCase()
  if (lower === ' ') return 'space'
  if (lower === 'esc') return 'escape'
  if (lower === 'return') return 'enter'
  if (lower === 'plus') return '+'
  if (lower === 'minus') return '-'
  return lower
}

export function normalizeStroke(stroke: ShortcutStroke): string {
  const mods: string[] = []
  if (stroke.alt) mods.push('alt')
  if (stroke.ctrl) mods.push('ctrl')
  if (stroke.shift) mods.push('shift')
  return [...mods, normalizeShortcutKey(stroke.key)].join('+')
}

export function formatShortcut(spec: string | undefined): string {
  return splitShortcutSequence(spec)
    .map((stroke) => stroke.map(formatShortcutPart).join('+'))
    .join(' ')
}

export function formatShortcutPart(part: string): string {
  const lower = part.toLowerCase()
  if (lower === 'ctrl' || lower === 'control' || lower === 'cmd' || lower === 'meta') return 'Ctrl'
  if (lower === 'shift') return 'Shift'
  if (lower === 'alt' || lower === 'option') return 'Alt'
  if (lower === 'escape') return 'Esc'
  if (lower === 'enter') return 'Enter'
  if (lower === 'space') return 'Space'
  if (/^f\d+$/i.test(part)) return part.toUpperCase()
  if (part.length === 1) return part.toUpperCase()
  return part[0].toUpperCase() + part.slice(1)
}

export function splitShortcut(spec: string | undefined): string[] {
  const [firstStroke] = splitShortcutSequence(spec)
  return firstStroke?.map(formatShortcutPart) ?? []
}

export function shortcutEquals(a: string, b: string): boolean {
  return normalizeShortcut(a) === normalizeShortcut(b)
}
