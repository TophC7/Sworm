// Resolve the logical key name of a `KeyboardEvent` defensively.
//
// `KeyboardEvent.key` is the layout-aware character produced by the
// keystroke (e.g. `'a'`, `'Enter'`, `'Tab'`). On WebKitGTK, `key` can
// regress to `'Unidentified'` when Shift is held with non-character
// keys; observed for `Shift+Tab` and `Shift+Enter`. `KeyboardEvent.code`
// is the physical-key identifier (`'Tab'`, `'Enter'`, `'KeyA'`) and is
// unaffected by the regression.
//
// `logicalKey` returns `event.key` when it is meaningful, and falls
// back to a code-derived name for the standard non-character keys
// otherwise. Use it anywhere we previously did `ev.key === 'Tab'` or
// similar — the comparison stays the same, the fallback just keeps
// working through `'Unidentified'` deliveries.

const CODE_TO_KEY: Record<string, string> = {
  Tab: 'Tab',
  Enter: 'Enter',
  NumpadEnter: 'Enter',
  Escape: 'Escape',
  Backspace: 'Backspace',
  Delete: 'Delete',
  ArrowUp: 'ArrowUp',
  ArrowDown: 'ArrowDown',
  ArrowLeft: 'ArrowLeft',
  ArrowRight: 'ArrowRight',
  Home: 'Home',
  End: 'End',
  PageUp: 'PageUp',
  PageDown: 'PageDown',
  Space: ' '
}

export function logicalKey(event: KeyboardEvent): string {
  const key = event.key
  if (key && key !== 'Unidentified') return key
  return CODE_TO_KEY[event.code] ?? key ?? ''
}
