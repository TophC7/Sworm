// Keyed reactive Map: each key has its own version counter that
// reactive consumers can subscribe to without seeing notifications
// from sibling keys. The map's data is a plain Map; reactivity comes
// from per-key Signal instances.
//
// Use over a single broadcast `version = $state(0)` whenever a
// notification is meaningful to one subset of consumers (e.g. one row
// in a list of hundreds). Broadcasting wakes everyone; per-key wakes
// only the row whose key bumped.
//
// Usage from a `.svelte.ts` module or component:
//
//   const store = new ReactiveMap<string, Entry>()
//
//   // Reactive read for one key:
//   $effect(() => {
//     void store.keyVersion(path)
//     const entry = store.get(path)
//     ...
//   })
//
//   // Reactive read for any change:
//   $effect(() => {
//     void store.version
//     for (const e of store.values()) ...
//   })

class Signal {
  v = $state(0)
}

export class ReactiveMap<K, V> {
  private data = new Map<K, V>()
  private signals = new Map<K, Signal>()
  // Broadcast: ticks on any structural mutation (set/delete) so
  // consumers reading `version` see all changes.
  private broadcast = new Signal()

  private signal(key: K): Signal {
    let s = this.signals.get(key)
    if (!s) {
      s = new Signal()
      this.signals.set(key, s)
    }
    return s
  }

  // Plain Map.get; not reactive on its own. Pair with keyVersion(key)
  // or `version` inside an effect to subscribe.
  get(key: K): V | undefined {
    return this.data.get(key)
  }

  has(key: K): boolean {
    return this.data.has(key)
  }

  set(key: K, value: V): void {
    this.data.set(key, value)
    this.signal(key).v++
    this.broadcast.v++
  }

  delete(key: K): boolean {
    const had = this.data.delete(key)
    if (had) {
      // Tick the key's signal one last time before dropping it so any
      // currently-subscribed effect re-runs and observes `has(key) === false`.
      const s = this.signals.get(key)
      if (s) s.v++
      this.signals.delete(key)
      this.broadcast.v++
    }
    return had
  }

  get size(): number {
    return this.data.size
  }

  entries(): IterableIterator<[K, V]> {
    return this.data.entries()
  }

  values(): IterableIterator<V> {
    return this.data.values()
  }

  keys(): IterableIterator<K> {
    return this.data.keys()
  }

  // Tick this key's signal so consumers reading keyVersion(key) re-fire.
  // Use whenever an in-place mutation on the entry needs to wake
  // subscribers (e.g. `entry.activity = 'working'` then `bumpKey(id)`).
  bumpKey(key: K): void {
    this.signal(key).v++
  }

  // Reactive read: subscribe to this key's per-key version.
  keyVersion(key: K): number {
    return this.signal(key).v
  }

  // Tick the broadcast signal alone (no per-key tick). Use when
  // structural state outside any single key changed but no key was
  // mutated (rare; most call sites should use `bumpKey` or `set`).
  bumpAll(): void {
    this.broadcast.v++
  }

  // Reactive read: subscribe to the broadcast version.
  get version(): number {
    return this.broadcast.v
  }

  clear(): void {
    if (this.data.size === 0) return
    this.data.clear()
    this.signals.clear()
    this.broadcast.v++
  }
}
