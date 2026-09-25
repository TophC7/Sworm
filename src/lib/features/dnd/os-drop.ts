import { platform, requireNative } from '$lib/platform'
import type { Unsubscribe } from '$lib/api/transport'
import type { DragPayload } from '$lib/features/dnd/payload'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'

let unlisten: Unsubscribe | null = null
let hoverPayload: DragPayload | null = null
let lastPaths: string[] = []
let mountCount = 0
let registering: Promise<void> | null = null

export async function initOsDrop(): Promise<void> {
  if (!platform.capabilities.osDragDrop) return
  mountCount += 1
  if (unlisten) return
  registering ??= register()
  await registering
}

async function register(): Promise<void> {
  try {
    const stop = await requireNative().osDrop.onEvent((event) => {
      const deviceScale = window.devicePixelRatio || 1

      if (event.type === 'enter') {
        lastPaths = event.paths
        if (lastPaths.length === 0) return
        hoverPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...lastPaths] }]
        }
        LocalTransfer.set(hoverPayload)
        DropRegistry.hoverAt(event.position.x / deviceScale, event.position.y / deviceScale, hoverPayload)
        return
      }

      if (event.type === 'over') {
        if (lastPaths.length === 0) return
        hoverPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...lastPaths] }]
        }
        LocalTransfer.set(hoverPayload)
        DropRegistry.hoverAt(event.position.x / deviceScale, event.position.y / deviceScale, hoverPayload)
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
  if (!platform.capabilities.osDragDrop) return
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
  hoverPayload = null
  lastPaths = []
}
