import { join } from '@tauri-apps/api/path'
import { backend } from '$lib/api/backend'
import { DND_MIME, type DragPayload } from '$lib/features/dnd/payload'
import { createHoverStore } from '$lib/features/dnd/hover-state.svelte'
import { dragObserver, frameAt } from '$lib/features/dnd/observer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import type { TabId } from '$lib/features/workbench/model'

interface TerminalDropObserverArgs {
  tabId: TabId
  folderPath: string
  canAcceptDrop?: () => boolean
  onInsertText: (text: string) => void
}

const hoverStore = createHoverStore<true>()

function setHover(tabId: TabId): void {
  hoverStore.set(tabId, true)
}

function clearHover(tabId: TabId): void {
  hoverStore.clear(tabId)
}

function canAccept(payload: DragPayload | null): boolean {
  if (!payload) return false
  return payload.items.some((item) => {
    if (item.kind === 'file') return !item.isDir
    if (item.kind === 'os-files') return item.paths.length > 0
    return false
  })
}

/**
 * Absolute paths for the payload, or `null` when a file item belongs
 * to another folder — its path is relative to a different workspace
 * root, so inserting it into this shell would be wrong.
 */
async function collectPathsFromPayload(payload: DragPayload, folderPath: string): Promise<string[] | null> {
  const paths: string[] = []
  for (const item of payload.items) {
    if (item.kind === 'file' && !item.isDir) {
      if (item.folderPath !== folderPath) return null
      paths.push(await join(folderPath, item.path))
    } else if (item.kind === 'os-files') {
      paths.push(...item.paths)
    }
  }
  return paths
}

function dropEnabled(args: TerminalDropObserverArgs): boolean {
  return args.canAcceptDrop?.() ?? true
}

async function collectImagePathsFromEvent(event: DragEvent): Promise<string[]> {
  const files = Array.from(event.dataTransfer?.files ?? [])
  const images = files.filter((file) => file.type.startsWith('image/'))
  if (images.length === 0) return []

  const tempPaths: string[] = []
  for (const image of images) {
    const bytes = new Uint8Array(await image.arrayBuffer())
    const path = await backend.dnd.saveDroppedBytes(bytes, image.name || 'dropped-image.png')
    tempPaths.push(path)
  }
  return tempPaths
}

/** Shared drop handler for both the DOM observer and the DropRegistry path. */
async function insertFromPayload(args: TerminalDropObserverArgs, payload: DragPayload, event?: DragEvent) {
  try {
    const payloadPaths = await collectPathsFromPayload(payload, args.folderPath)
    if (payloadPaths === null) {
      notify.warning('Different folder')
      return
    }
    const insertPaths = payloadPaths.length > 0 || !event ? payloadPaths : await collectImagePathsFromEvent(event)
    const unique = Array.from(new Set(insertPaths))
    if (unique.length === 0) return
    args.onInsertText(`${unique.map((path) => preparePathForShell(path)).join(' ')} `)
  } catch (error) {
    notify.error('Terminal drop failed', getErrorMessage(error))
  }
}

function isCenterDropFrame(frame: { localX: number; localY: number; width: number; height: number }): boolean {
  const edgeX = frame.width * 0.15
  const edgeY = frame.height * 0.15
  return (
    frame.localX >= edgeX &&
    frame.localX <= frame.width - edgeX &&
    frame.localY >= edgeY &&
    frame.localY <= frame.height - edgeY
  )
}

function isCenterDropPoint(element: HTMLElement, clientX: number, clientY: number): boolean {
  const frame = frameAt(element, clientX, clientY)
  return frame ? isCenterDropFrame(frame) : false
}

export function terminalDropObserver(args: TerminalDropObserverArgs) {
  const observer = dragObserver({
    accept: (payload, types) => {
      if (!dropEnabled(args)) return false
      if (payload) return canAccept(payload)
      return types.includes(DND_MIME.SWORM_FILE) || types.includes(DND_MIME.FILES) || types.includes(DND_MIME.TEXT)
    },
    onOver: (_event, frame) => {
      if (!dropEnabled(args) || !isCenterDropFrame(frame)) {
        clearHover(args.tabId)
        return
      }
      setHover(args.tabId)
    },
    onLeave: () => {
      clearHover(args.tabId)
    },
    onDrop: async (event, payload, frame) => {
      clearHover(args.tabId)
      if (!dropEnabled(args) || (frame && !isCenterDropFrame(frame))) return
      await insertFromPayload(args, payload, event)
    }
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const onTextDrop = (event: DragEvent) => {
      const transfer = event.dataTransfer
      if (
        !transfer ||
        transfer.getData(DND_MIME.SWORM_ITEM) ||
        !transfer.types.includes(DND_MIME.TEXT) ||
        !dropEnabled(args) ||
        !isCenterDropPoint(element, event.clientX, event.clientY)
      ) {
        return
      }
      const path = transfer.getData(DND_MIME.TEXT).trim()
      if (!path) return
      event.preventDefault()
      clearHover(args.tabId)
      args.onInsertText(`${preparePathForShell(path)} `)
    }
    element.addEventListener('drop', onTextDrop)
    const disposeRegistry = DropRegistry.register({
      id: `terminal:${args.tabId}`,
      element,
      accept: (payload) => dropEnabled(args) && canAccept(payload),
      hitTest: (_payload, clientX, clientY) => isCenterDropPoint(element, clientX, clientY),
      hover: () => {
        setHover(args.tabId)
      },
      leave: () => {
        clearHover(args.tabId)
      },
      dispatch: async (payload) => {
        clearHover(args.tabId)
        if (!dropEnabled(args)) return
        await insertFromPayload(args, payload)
      }
    })

    return () => {
      element.removeEventListener('drop', onTextDrop)
      disposeRegistry()
      clearHover(args.tabId)
      disposeObserver()
    }
  }
}

export function isTerminalDropActive(tabId: TabId): boolean {
  return hoverStore.has(tabId)
}

export function preparePathForShell(path: string, shell: 'posix' | 'powershell' = 'posix'): string {
  if (shell === 'powershell') {
    return `'${path.replaceAll("'", "''")}'`
  }
  return `'${path.replaceAll("'", "'\\''")}'`
}
