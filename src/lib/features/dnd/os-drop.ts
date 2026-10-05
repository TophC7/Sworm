import { platform, type NativePlatform } from '$lib/platform'
import type { Unsubscribe } from '$lib/api/transport'
import type { DragPayload } from '$lib/features/dnd/payload'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'

let unlisten: Unsubscribe | null = null
let lastPaths: string[] = []
let mountCount = 0
let registering: Promise<void> | null = null

export async function initOsDrop(): Promise<void> {
  const native = platform.native
  if (!native) return
  mountCount += 1
  if (unlisten) return
  registering ??= register(native)
  await registering
}

async function register(native: NativePlatform): Promise<void> {
  try {
    const stop = await native.osDrop.onEvent((event) => {
      const deviceScale = window.devicePixelRatio || 1

      if (event.type === 'enter') lastPaths = event.paths
      if (event.type === 'enter' || event.type === 'over') {
        if (lastPaths.length === 0) return
        const payload: DragPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...lastPaths] }]
        }
        LocalTransfer.set(payload)
        DropRegistry.hoverAt(event.position.x / deviceScale, event.position.y / deviceScale, payload)
        return
      }

      if (event.type === 'drop') {
        const payload: DragPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...event.paths] }]
        }
        const clientX = event.position.x / deviceScale
        const clientY = event.position.y / deviceScale
        void DropRegistry.dispatchAt(clientX, clientY, payload)
        clearHoverState()
        return
      }

      clearHoverState()
    })
    if (mountCount === 0) {
      stop()
      clearHoverState()
    } else unlisten = stop
  } catch (error) {
    mountCount = 0
    throw error
  } finally {
    registering = null
  }
}

export function disposeOsDrop(): void {
  mountCount = Math.max(0, mountCount - 1)
  if (mountCount > 0) return
  if (!unlisten) return
  unlisten()
  unlisten = null
  clearHoverState()
}

function clearHoverState(): void {
  DropRegistry.clearHover()
  LocalTransfer.clear()
  lastPaths = []
}
