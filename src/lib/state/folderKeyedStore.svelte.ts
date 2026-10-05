export interface FolderPollOptions {
  intervalMs: number
  tick: (folderPath: string) => void | Promise<void>
}

interface Poller {
  interval: ReturnType<typeof setInterval>
  refs: number
}

/** Create folder-keyed reactive state with refcounted polling. Keys are canonical folder paths. */
export function createFolderKeyedStore<T extends object>() {
  let entries = $state<Map<string, T>>(new Map())
  const pollers = new Map<string, Poller>()
  // Retain generations after deletion so reopening cannot accept stale work.
  const generations = new Map<string, number>()

  function get(folderPath: string): T | undefined {
    return entries.get(folderPath)
  }

  function has(folderPath: string): boolean {
    return entries.has(folderPath)
  }

  function set(folderPath: string, entry: T) {
    if (entries.get(folderPath) === entry) return
    const next = new Map(entries)
    next.set(folderPath, entry)
    entries = next
  }

  function generation(folderPath: string): number {
    return generations.get(folderPath) ?? 0
  }

  function patch(folderPath: string, patch: Partial<T>) {
    const current = entries.get(folderPath)
    if (!current) return
    const keys = Object.keys(patch) as (keyof T)[]
    if (keys.every((key) => Object.is(current[key], patch[key]))) return
    set(folderPath, { ...current, ...patch })
  }

  function startPolling(folderPath: string, options: FolderPollOptions) {
    const current = pollers.get(folderPath)
    if (current) {
      current.refs += 1
      return
    }
    const interval = setInterval(() => void options.tick(folderPath), options.intervalMs)
    pollers.set(folderPath, { interval, refs: 1 })
  }

  function stopPolling(folderPath: string) {
    const current = pollers.get(folderPath)
    if (!current) return
    current.refs -= 1
    if (current.refs > 0) return
    clearInterval(current.interval)
    pollers.delete(folderPath)
  }

  function stopAllPolling(folderPath: string) {
    const current = pollers.get(folderPath)
    if (!current) return
    clearInterval(current.interval)
    pollers.delete(folderPath)
  }

  /** Fence pending work, drop the entry and stop every poller for `folderPath`. */
  function del(folderPath: string) {
    generations.set(folderPath, generation(folderPath) + 1)
    stopAllPolling(folderPath)
    if (!entries.has(folderPath)) return
    const next = new Map(entries)
    next.delete(folderPath)
    entries = next
  }

  return {
    get,
    has,
    set,
    generation,
    patch,
    startPolling,
    stopPolling,
    stopAllPolling,
    delete: del
  }
}
