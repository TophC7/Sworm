/**
 * Reactive keyed store used by drop adapters to track which of their
 * registered targets (panes, directories, git zones, terminals) is
 * currently hovered. Writes are deduped — setting the same value
 * doesn't trigger downstream `$derived` consumers, so this is safe to
 * call on every `dragover` tick.
 */
export interface HoverStore<T> {
  has(key: string): boolean
  get(key: string): T | undefined
  set(key: string, value: T, equals?: (a: T, b: T) => boolean): void
  clear(key: string): void
  clearByPrefix(prefix: string): void
}

export function createHoverStore<T>(): HoverStore<T> {
  let entries = $state<Record<string, T>>({})

  return {
    has(key) {
      return key in entries
    },
    get(key) {
      return entries[key]
    },
    set(key, value, equals) {
      const prev = entries[key]
      if (prev !== undefined && (equals ? equals(prev, value) : prev === value)) return
      entries[key] = value
    },
    clear(key) {
      if (!(key in entries)) return
      const { [key]: _removed, ...rest } = entries
      entries = rest
    },
    clearByPrefix(prefix) {
      let changed = false
      const next: Record<string, T> = {}
      for (const [k, v] of Object.entries(entries)) {
        if (k.startsWith(prefix)) {
          changed = true
        } else {
          next[k] = v
        }
      }
      if (changed) entries = next
    }
  }
}
