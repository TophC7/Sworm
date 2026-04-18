import type { UnlistenFn } from '@tauri-apps/api/event'
import type { DragPayload } from '$lib/dnd/payload'
import { LocalTransfer } from '$lib/dnd/transfer.svelte'
import { DropRegistry } from '$lib/dnd/registry.svelte'

let unlisten: UnlistenFn | null = null
let hoverPayload: DragPayload | null = null
let lastPaths: string[] = []
let mountCount = 0
let bridgeAvailable = true

export async function initTauriOsDrop(): Promise<void> {
  mountCount += 1
  if (!bridgeAvailable) return
  if (unlisten) return
  try {
    const { getCurrentWebview } = await import('@tauri-apps/api/webview')
    unlisten = await getCurrentWebview().onDragDropEvent((event) => {
      const deviceScale = window.devicePixelRatio || 1

      if (event.payload.type === 'enter') {
        lastPaths = event.payload.paths
        if (lastPaths.length === 0) return
        hoverPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...lastPaths] }]
        }
        LocalTransfer.set(hoverPayload)
        DropRegistry.hoverAt(
          event.payload.position.x / deviceScale,
          event.payload.position.y / deviceScale,
          hoverPayload
        )
        return
      }

      if (event.payload.type === 'over') {
        if (lastPaths.length === 0) return
        hoverPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...lastPaths] }]
        }
        LocalTransfer.set(hoverPayload)
        DropRegistry.hoverAt(
          event.payload.position.x / deviceScale,
          event.payload.position.y / deviceScale,
          hoverPayload
        )
        return
      }

      if (event.payload.type === 'drop') {
        const payload: DragPayload = {
          source: 'external',
          items: [{ kind: 'os-files', paths: [...event.payload.paths] }]
        }
        const clientX = event.payload.position.x / deviceScale
        const clientY = event.payload.position.y / deviceScale
        void DropRegistry.dispatchAt(clientX, clientY, payload)
        clearHoverState()
        return
      }

      clearHoverState()
    })
  } catch (error) {
    bridgeAvailable = false
    mountCount = Math.max(0, mountCount - 1)
    console.warn('Tauri OS drop bridge unavailable; continuing without OS drag-and-drop.', error)
  }
}

export function disposeTauriOsDrop(): void {
  if (!bridgeAvailable) return
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
