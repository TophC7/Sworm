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
  const timers = new Map<
    string,
    {
      timeout: ReturnType<typeof setTimeout>
      task: () => Promise<unknown>
    }
  >()
  let inFlight = 0
  let busy = false

  function settle() {
    const nextBusy = timers.size > 0 || inFlight > 0
    if (nextBusy !== busy) {
      busy = nextBusy
      opts.onBusyChange(busy)
    }
  }

  function runTask(key: string, task: () => Promise<unknown>) {
    timers.delete(key)
    inFlight += 1
    settle()
    void task()
      .catch(() => {
        // Callers surface their own errors (notify.error). This
        // guard just ensures an unexpected throw still settles the
        // busy flag instead of stranding the save indicator.
      })
      .finally(() => {
        inFlight = Math.max(0, inFlight - 1)
        settle()
      })
  }

  return {
    schedule(key, task) {
      const existing = timers.get(key)
      if (existing) clearTimeout(existing.timeout)
      const timeout = setTimeout(() => runTask(key, task), delay)
      timers.set(key, { timeout, task })
      settle()
    },

    dispose() {
      for (const [key, pending] of [...timers.entries()]) {
        clearTimeout(pending.timeout)
        runTask(key, pending.task)
      }
    }
  }
}
