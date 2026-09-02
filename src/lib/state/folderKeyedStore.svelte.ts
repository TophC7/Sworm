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

  function update(folderPath: string, updater: (current: T) => T) {
    const current = entries.get(folderPath)
    if (!current) return
    const next = updater(current)
    if (next === current) return
    set(folderPath, next)
  }

  function patch(folderPath: string, patch: Partial<T>) {
    update(folderPath, (current) => {
      const keys = Object.keys(patch) as (keyof T)[]
      if (keys.every((key) => Object.is(current[key], patch[key]))) {
        return current
      }
      return { ...current, ...patch }
    })
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

  /** Drop the entry and stop every poller for `folderPath`. No-op when unknown. */
  function del(folderPath: string) {
    stopAllPolling(folderPath)
    if (!entries.has(folderPath)) return
    const next = new Map(entries)
    next.delete(folderPath)
    entries = next
  }

  /** Drop every entry and stop every poller. */
  function clear() {
    for (const poller of pollers.values()) clearInterval(poller.interval)
    pollers.clear()
    if (entries.size === 0) return
    entries = new Map()
  }

  return {
    get,
    has,
    set,
    update,
    patch,
    startPolling,
    stopPolling,
    stopAllPolling,
    delete: del,
    clear
  }
}
