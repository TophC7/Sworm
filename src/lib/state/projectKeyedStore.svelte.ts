export interface ProjectPollOptions {
  intervalMs: number
  tick: (projectId: string, projectPath: string) => void | Promise<void>
}

interface Poller {
  interval: ReturnType<typeof setInterval>
  refs: number
}

/** Create project-keyed reactive state with path caching and refcounted polling. */
export function createProjectKeyedStore<T extends object>() {
  let entries = $state<Map<string, T>>(new Map())
  const projectPaths = new Map<string, string>()
  const pollers = new Map<string, Poller>()

  function get(projectId: string): T | undefined {
    return entries.get(projectId)
  }

  function has(projectId: string): boolean {
    return entries.has(projectId)
  }

  function set(projectId: string, entry: T) {
    if (entries.get(projectId) === entry) return
    const next = new Map(entries)
    next.set(projectId, entry)
    entries = next
  }

  function update(projectId: string, updater: (current: T) => T) {
    const current = entries.get(projectId)
    if (!current) return
    const next = updater(current)
    if (next === current) return
    set(projectId, next)
  }

  function patch(projectId: string, patch: Partial<T>) {
    update(projectId, (current) => {
      const keys = Object.keys(patch) as (keyof T)[]
      if (keys.every((key) => Object.is(current[key], patch[key]))) {
        return current
      }
      return { ...current, ...patch }
    })
  }

  function deleteEntry(projectId: string) {
    const next = new Map(entries)
    next.delete(projectId)
    entries = next
  }

  function setProjectPath(projectId: string, projectPath: string) {
    projectPaths.set(projectId, projectPath)
  }

  function getProjectPath(projectId: string): string | undefined {
    return projectPaths.get(projectId)
  }

  function resolveProjectPath(projectId: string, projectPath?: string): string | undefined {
    if (projectPath) {
      projectPaths.set(projectId, projectPath)
      return projectPath
    }
    return projectPaths.get(projectId)
  }

  function deleteProjectPath(projectId: string) {
    projectPaths.delete(projectId)
  }

  function startPolling(projectId: string, projectPath: string, options: ProjectPollOptions) {
    projectPaths.set(projectId, projectPath)

    const current = pollers.get(projectId)
    if (current) {
      current.refs += 1
      return
    }

    const run = () => {
      const path = projectPaths.get(projectId)
      if (!path) return
      void options.tick(projectId, path)
    }

    const interval = setInterval(run, options.intervalMs)
    pollers.set(projectId, { interval, refs: 1 })
  }

  function stopPolling(projectId: string) {
    const current = pollers.get(projectId)
    if (!current) return
    current.refs -= 1
    if (current.refs > 0) return
    clearInterval(current.interval)
    pollers.delete(projectId)
  }

  function stopAllPolling(projectId: string) {
    const current = pollers.get(projectId)
    if (!current) return
    clearInterval(current.interval)
    pollers.delete(projectId)
  }

  function clearFor(projectId: string) {
    stopAllPolling(projectId)
    deleteProjectPath(projectId)
    deleteEntry(projectId)
  }

  return {
    get,
    has,
    set,
    update,
    patch,
    delete: deleteEntry,
    clearFor,
    setProjectPath,
    getProjectPath,
    resolveProjectPath,
    deleteProjectPath,
    startPolling,
    stopPolling,
    stopAllPolling
  }
}
