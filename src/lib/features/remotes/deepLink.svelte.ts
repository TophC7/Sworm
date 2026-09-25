import { requireNative } from '$lib/platform'
import { notify } from '$lib/features/notifications/state.svelte'
import { openPairLink, parsePairLink } from './state.svelte'

let resolveReady: () => void
const ready = new Promise<void>((resolve) => {
  resolveReady = resolve
})

export function markDeepLinksReady(): void {
  resolveReady()
}

export function initDeepLinks(): () => void {
  let disposed = false
  let draining = false
  let wake = false
  let unlisten: (() => void) | undefined

  async function drain() {
    wake = true
    if (draining || disposed) return
    draining = true
    try {
      await ready
      while (wake && !disposed) {
        wake = false
        const links = await requireNative().deepLinks.take()
        for (const link of links) {
          if (parsePairLink(link)) openPairLink(link)
          else notify.error('Invalid pairing link', 'Request a fresh link from sworm-server pair.')
        }
      }
    } catch {
      notify.error('Could not receive links', 'Paste the pairing link into Settings → Remote Servers instead.')
    } finally {
      draining = false
    }
  }

  // Subscribe first, then atomically take the queue: startup and live delivery share one owner.
  const listener = requireNative().deepLinks.onOpen(() => {
    void drain()
  })
  void listener
    .then((cleanup) => {
      if (disposed) cleanup()
      else {
        unlisten = cleanup
        void drain()
      }
    })
    .catch(() => {
      if (!disposed)
        notify.error('Could not receive links', 'Paste the pairing link into Settings → Remote Servers instead.')
    })
  return () => {
    disposed = true
    unlisten?.()
  }
}
