// Places — where the user can go besides the current tab: paired servers
// with their connection state, and durable workbenches per host. Recent
// folders stay in `features/folders`. Live only while a watcher is mounted,
// because listing workbenches reaches every paired server.

import { backend } from '$lib/api/backend'
import { platform } from '$lib/platform'
import { getRemoteStatus, refreshRemoteStatus, watchRemoteStatuses } from '$lib/features/remotes/status.svelte'
import type { RemoteStatus, WorkbenchInfo } from '$lib/types/backend'
import { logClientError } from '$lib/utils/client-error'
import { basename } from '$lib/utils/paths'

export type ServerState = RemoteStatus['state'] | 'checking'

export interface ServerPlace {
  name: string
  state: ServerState
  lastError: string | null
}

/** Workbenches of one host; `server` is unset for the page's own host on web. */
export interface WorkbenchSection {
  server?: string
  workbenches: WorkbenchInfo[]
}

let serverNames = $state<string[]>([])
let sections = $state<WorkbenchSection[]>([])
let watchers = 0
let teardown: (() => void) | null = null
let refreshing: Promise<void> | null = null
let queued = false
let generation = 0

/** Paired servers in settings order; empty where this client has no remote hosts. */
export function getServers(): ServerPlace[] {
  return serverNames.map((name) => ({
    name,
    state: getRemoteStatus(name)?.state ?? 'checking',
    lastError: getRemoteStatus(name)?.last_error ?? null
  }))
}

export function getWorkbenchSections(): WorkbenchSection[] {
  return sections
}

/** Workbench name for lists: its tab folders' names. */
export function workbenchTitle(workbench: WorkbenchInfo): string {
  return [...new Set(workbench.folders.map(basename))].join(' · ') || 'Empty Workbench'
}

/** Where a workbench lives, what it runs, and who holds it. */
export function workbenchDetail(workbench: WorkbenchInfo, server?: string): string {
  return [
    server,
    workbench.running.length > 0 ? `${workbench.running.length} running` : null,
    workbench.connected ? `in ${workbench.client ?? 'another window'}` : null
  ]
    .filter(Boolean)
    .join(' · ')
}

// An empty workbench from another controller, or the one this page is, is noise.
function listable(workbench: WorkbenchInfo): boolean {
  return (
    !workbench.yours &&
    workbench.id !== platform.workbench.id &&
    (workbench.folders.length > 0 || workbench.running.length > 0)
  )
}

async function load(current: number): Promise<void> {
  const native = platform.native
  if (native) {
    const names = Object.keys((await backend.settings.getEffective()).settings.remotes)
    if (current !== generation) return
    serverNames = names
    const [nextSections] = await Promise.all([
      Promise.all(
        names.map(async (server) => {
          try {
            return { server, workbenches: (await backend.workbenches.list(server)).filter(listable) }
          } catch (error) {
            logClientError(`Failed to list workbenches on ${server}`, { error })
            return { server, workbenches: [] }
          }
        })
      ),
      Promise.all(names.map(refreshRemoteStatus))
    ])
    if (current === generation) sections = nextSections
  } else if (platform.workbench.takeOver) {
    const workbenches = (await backend.workbenches.list()).filter(listable)
    if (current === generation) sections = [{ workbenches }]
  }
}

/** Re-read servers and workbenches; overlapping calls coalesce into one trailing pass. */
export function refreshPlaces(): Promise<void> {
  if (refreshing) {
    queued = true
    return refreshing
  }
  refreshing = (async () => {
    do {
      queued = false
      try {
        await load(++generation)
      } catch (error) {
        logClientError('Failed to list places', { error })
      }
    } while (queued && watchers > 0)
    refreshing = null
  })()
  return refreshing
}

/**
 * Keep places live while the caller is mounted; returns the disposer.
 * Use from `onMount` or an `$effect` so the subscription follows the view.
 */
export function watchPlaces(): () => void {
  watchers++
  if (watchers === 1) teardown = subscribe()
  return () => {
    watchers--
    if (watchers > 0) return
    teardown?.()
    teardown = null
    generation++
  }
}

function subscribe(): () => void {
  let disposed = false
  const stopStatuses = watchRemoteStatuses()
  const refresh = () => {
    if (!disposed) void refreshPlaces()
  }
  const native = platform.native
  const listeners = [
    backend.workbenches.onChanged(refresh),
    ...(native
      ? [
          backend.settings.onChanged(({ layer }) => {
            if (layer === 'global') refresh()
          })
        ]
      : [])
  ]
  window.addEventListener('focus', refresh)
  // Fetch after subscribing so no transition lands between the two.
  void Promise.allSettled(listeners).then(refresh)
  return () => {
    disposed = true
    stopStatuses()
    window.removeEventListener('focus', refresh)
    for (const listener of listeners) void listener.then((stop) => stop()).catch(() => {})
  }
}
