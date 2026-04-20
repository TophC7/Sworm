// Debounced saver shared by settings views. Each schedule replaces any
// pending timer for the same key, so fast typing collapses into a
// single flush. `onBusyChange` fires exactly once per transition
// between "timer or save pending" and "idle", so the parent dialog can
// aggregate status across views without counter leaks.

export interface AutoSaver {
  schedule(key: string, task: () => Promise<unknown>): void
  dispose(): void
}

export function createAutoSaver(opts: { delayMs?: number; onBusyChange: (busy: boolean) => void }): AutoSaver {
  const delay = opts.delayMs ?? 400
  const timers = new Map<string, ReturnType<typeof setTimeout>>()
  const inFlight = new Set<string>()
  let busy = false

  function settle() {
    const nextBusy = timers.size > 0 || inFlight.size > 0
    if (nextBusy !== busy) {
      busy = nextBusy
      opts.onBusyChange(busy)
    }
  }

  return {
    schedule(key, task) {
      const existing = timers.get(key)
      if (existing) clearTimeout(existing)
      const timer = setTimeout(async () => {
        timers.delete(key)
        inFlight.add(key)
        settle()
        try {
          await task()
        } catch {
          // Callers surface their own errors (notify.error). This
          // guard just ensures an unexpected throw still settles the
          // busy flag instead of stranding the save indicator.
        } finally {
          inFlight.delete(key)
          settle()
        }
      }, delay)
      timers.set(key, timer)
      settle()
    },

    dispose() {
      for (const t of timers.values()) clearTimeout(t)
      timers.clear()
      // In-flight saves continue; their own finally blocks will
      // settle. Drop them from our tracking so the next settle()
      // call reports idle immediately.
      inFlight.clear()
      settle()
    }
  }
}
