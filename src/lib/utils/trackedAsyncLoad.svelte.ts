// Async-load helper for views that fetch their content when a scalar
// key (commit hash, stash index, change signature) flips.
//
// The shape three diff views were reimplementing by hand:
//   1. Closure variable tracking the last-loaded key.
//   2. `$effect` that compares the current key, resets state if it
//      changed, and kicks off the async load.
//   3. The async load early-returns on completion if the key moved
//      again while the fetch was in flight, so a slow response for
//      an old key can't stomp a newer result.
//
// Usage from a component:
//
//   const loader = createTrackedAsyncLoad<string>()
//   $effect(() => {
//     const hash = commitHash
//     loader.run(hash, async (isCurrent) => {
//       detail = null
//       files = []
//       const d = await backend.git.getCommitDetail(path, hash)
//       if (!isCurrent()) return
//       detail = d
//     })
//   })
//   let loading = $derived(loader.loading)
//
// `isCurrent()` returns true iff the key hasn't moved since this run
// started. Callers should gate every state write on it after an await.

export interface TrackedAsyncLoad<K> {
  /** `true` while the most recent run is in flight. Reactive. */
  readonly loading: boolean
  /**
   * Invoke with the current key and a load function. If `key` matches
   * the last-seen value, the call is a no-op. Otherwise the previous
   * run is logically abandoned (its `isCurrent()` will return false)
   * and `load` starts. `load` is responsible for setting whatever
   * component state the view needs — keep state writes gated on
   * `isCurrent()` after every await.
   */
  run(key: K, load: (isCurrent: () => boolean) => Promise<void>): void
}

export function createTrackedAsyncLoad<K>(): TrackedAsyncLoad<K> {
  const state = $state({ loading: false })
  let currentKey: K | symbol = UNLOADED

  return {
    get loading() {
      return state.loading
    },
    run(key, load) {
      if (key === currentKey) return
      currentKey = key
      const isCurrent = () => currentKey === key
      state.loading = true
      void (async () => {
        try {
          await load(isCurrent)
        } catch {
          // Swallow — callers already reset their own error state on
          // the leading edge of `load`. Rethrow would fire an
          // unhandledrejection the view can't act on.
        } finally {
          if (isCurrent()) state.loading = false
        }
      })()
    }
  }
}

// Sentinel for "no key has ever been loaded." Kept distinct from any
// caller-supplied value (including `null` or `undefined`) so the very
// first `run` always fires.
const UNLOADED = Symbol('trackedAsyncLoad.unloaded')
