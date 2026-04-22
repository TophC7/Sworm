import { join } from '@tauri-apps/api/path'
import { backend } from '$lib/api/backend'
import { DND_MIME, type DragPayload } from '$lib/features/dnd/payload'
import { createHoverStore } from '$lib/features/dnd/hover-state.svelte'
import { computeZone } from '$lib/features/dnd/overlay'
import { dragObserver, frameAt } from '$lib/features/dnd/observer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'
import { notify } from '$lib/features/notifications/state.svelte'

interface TerminalDropObserverArgs {
  sessionId: string
  projectId: string
  projectPath: string
  canAcceptDrop?: () => boolean
  onInsertText: (text: string) => void
}

const hoverStore = createHoverStore<true>()

function setHover(sessionId: string): void {
  hoverStore.set(sessionId, true)
}

function clearHover(sessionId: string): void {
  hoverStore.clear(sessionId)
}

function canAccept(payload: DragPayload | null, projectId: string): boolean {
  if (!payload) return false
  return payload.items.some((item) => {
    if (item.kind === 'file') return item.projectId === projectId && !item.isDir
    if (item.kind === 'os-files') return item.paths.length > 0
    return false
  })
}

async function collectPathsFromPayload(
  payload: DragPayload,
  projectPath: string,
  projectId: string
): Promise<string[]> {
  const paths: string[] = []
  for (const item of payload.items) {
    if (item.kind === 'file' && item.projectId === projectId && !item.isDir) {
      paths.push(await join(projectPath, item.path))
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

function uniquePaths(paths: string[]): string[] {
  return Array.from(new Set(paths))
}

function emitPaths(paths: string[], onInsertText: (text: string) => void): void {
  if (paths.length === 0) return
  const quoted = paths.map((path) => preparePathForShell(path)).join(' ')
  onInsertText(`${quoted} `)
}

function isCenterDropFrame(frame: { localX: number; localY: number; width: number; height: number }): boolean {
  return (
    computeZone(frame.localX, frame.localY, frame.width, frame.height, {
      allowSplit: true,
      edgeMarginRatio: 0.15
    }) === 'merge'
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
      if (payload) return canAccept(payload, args.projectId)
      return types.includes(DND_MIME.FILES)
    },
    onOver: (_event, frame) => {
      if (!dropEnabled(args) || !isCenterDropFrame(frame)) {
        clearHover(args.sessionId)
        return
      }
      setHover(args.sessionId)
    },
    onLeave: () => {
      clearHover(args.sessionId)
    },
    onDrop: async (event, payload, frame) => {
      clearHover(args.sessionId)
      if (!dropEnabled(args) || (frame && !isCenterDropFrame(frame))) return
      try {
        const payloadPaths = await collectPathsFromPayload(payload, args.projectPath, args.projectId)
        const insertPaths = payloadPaths.length > 0 ? payloadPaths : await collectImagePathsFromEvent(event)
        emitPaths(uniquePaths(insertPaths), args.onInsertText)
      } catch (error) {
        notify.error('Terminal drop failed', error instanceof Error ? error.message : String(error))
      }
    }
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeRegistry = DropRegistry.register({
      id: `terminal:${args.sessionId}`,
      element,
      accept: (payload) => dropEnabled(args) && canAccept(payload, args.projectId),
      hitTest: (_payload, clientX, clientY) => isCenterDropPoint(element, clientX, clientY),
      hover: () => {
        setHover(args.sessionId)
      },
      leave: () => {
        clearHover(args.sessionId)
      },
      dispatch: async (payload) => {
        clearHover(args.sessionId)
        if (!dropEnabled(args)) return
        try {
          const payloadPaths = await collectPathsFromPayload(payload, args.projectPath, args.projectId)
          emitPaths(uniquePaths(payloadPaths), args.onInsertText)
        } catch (error) {
          notify.error('Terminal drop failed', error instanceof Error ? error.message : String(error))
        }
      }
    })

    return () => {
      disposeRegistry()
      clearHover(args.sessionId)
      disposeObserver()
    }
  }
}

export function isTerminalDropActive(sessionId: string): boolean {
  return hoverStore.has(sessionId)
}

export function preparePathForShell(path: string, shell: 'posix' | 'powershell' = 'posix'): string {
  if (shell === 'powershell') {
    return `'${path.replaceAll("'", "''")}'`
  }
  return `'${path.replaceAll("'", "'\\''")}'`
}
