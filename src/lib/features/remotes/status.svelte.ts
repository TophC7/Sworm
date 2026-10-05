import { platform } from '$lib/platform'
import type { RemoteStatus } from '$lib/types/backend'
import { getErrorMessage } from '$lib/utils/client-error'

let statuses = $state<Record<string, RemoteStatus>>({})
const revisions = new Map<string, number>()
let watchers = 0
let generation = 0
let ready: Promise<void> | null = null
let unlisten: (() => void) | null = null

/** Share one native status subscription for all mounted consumers. */
export function watchRemoteStatuses(): () => void {
  const native = platform.native
  if (!native) return () => {}
  if (++watchers === 1) {
    const current = generation
    ready = Promise.resolve()
      .then(() => native.remotes.onStatus(({ server, ...status }) => {
        if (current !== generation || watchers === 0) return
        revisions.set(server, (revisions.get(server) ?? 0) + 1)
        statuses[server] = status
      }))
      .then((stop) => {
        if (current !== generation || watchers === 0) stop()
        else unlisten = stop
      })
      .catch((error) => {
        console.error('Remote status listener failed:', error)
        throw error
      })
    // Keep readiness rejected for refresh callers without an unhandled rejection.
    void ready.catch(() => {})
  }
  let stopped = false
  return () => {
    if (stopped) return
    stopped = true
    if (--watchers > 0) return
    generation++
    unlisten?.()
    unlisten = null
    ready = null
  }
}

/** Revalidate cached state without overwriting a newer status event. */
export async function refreshRemoteStatus(server: string): Promise<void> {
  const native = platform.native
  if (!native || !ready) return
  const current = generation
  let revision = revisions.get(server) ?? 0
  try {
    await ready
    if (current !== generation) return
    revision = revisions.get(server) ?? 0
    const status = await native.remotes.status(server)
    if (current === generation && revision === (revisions.get(server) ?? 0)) statuses[server] = status
  } catch (error) {
    if (current === generation && revision === (revisions.get(server) ?? 0)) {
      statuses[server] = { connected: false, state: 'error', last_error: getErrorMessage(error) }
    }
  }
}

export function getRemoteStatus(server: string): RemoteStatus | null {
  return statuses[server] ?? null
}
