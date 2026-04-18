import { DND_MIME, type DragPayload, stampDataTransfer } from '$lib/dnd/payload'
import { createHoverStore } from '$lib/dnd/hover-state.svelte'
import { delayedDragHover } from '$lib/dnd/delayed-hover'
import { dragObserver } from '$lib/dnd/observer.svelte'
import { DropRegistry } from '$lib/dnd/registry.svelte'
import { LocalTransfer } from '$lib/dnd/transfer.svelte'
import type { FileTreeNode } from '$lib/utils/fileTree'

interface FileTreeSourceArgs {
  projectId: string
  node: FileTreeNode<{ path: string }>
}

interface FileTreeDirectoryTargetArgs {
  projectId: string
  directoryPath: string
  onDrop: (payload: DragPayload) => void | Promise<void>
  onHoverExpand?: () => void
}

const directoryStore = createHoverStore<true>()

function directoryKey(projectId: string, path: string): string {
  return `${projectId}:${path}`
}

function setDirectoryActive(projectId: string, path: string): void {
  directoryStore.set(directoryKey(projectId, path), true)
}

function clearDirectoryActive(projectId: string, path: string): void {
  directoryStore.clear(directoryKey(projectId, path))
}

function canAcceptDirectoryPayload(payload: DragPayload | null, projectId: string): boolean {
  if (!payload) return false
  return payload.items.some((item) => {
    if (item.kind === 'file') return item.projectId === projectId
    if (item.kind === 'os-files') return item.paths.length > 0
    return false
  })
}

export function fileTreeDragSource(args: FileTreeSourceArgs) {
  return (element: HTMLElement) => {
    const onDragStart = (event: DragEvent) => {
      const transfer = event.dataTransfer
      if (!transfer) {
        event.preventDefault()
        return
      }

      const payload: DragPayload = {
        source: 'internal',
        items: [
          {
            kind: 'file',
            path: args.node.path,
            isDir: args.node.type === 'directory',
            projectId: args.projectId
          }
        ]
      }

      LocalTransfer.set(payload)
      transfer.effectAllowed = 'move'
      stampDataTransfer(transfer, payload)
    }

    const onDragEnd = () => {
      LocalTransfer.clear()
      directoryStore.clearByPrefix(`${args.projectId}:`)
    }

    element.addEventListener('dragstart', onDragStart)
    element.addEventListener('dragend', onDragEnd)
    return () => {
      element.removeEventListener('dragstart', onDragStart)
      element.removeEventListener('dragend', onDragEnd)
    }
  }
}

export function fileTreeDirectoryDropTarget(args: FileTreeDirectoryTargetArgs) {
  const observer = dragObserver({
    accept: (payload, types) => {
      if (payload) return canAcceptDirectoryPayload(payload, args.projectId)
      return types.includes(DND_MIME.FILES)
    },
    onOver: () => {
      setDirectoryActive(args.projectId, args.directoryPath)
    },
    onLeave: () => {
      clearDirectoryActive(args.projectId, args.directoryPath)
    },
    onDrop: async (_event, payload) => {
      clearDirectoryActive(args.projectId, args.directoryPath)
      await args.onDrop(payload)
    }
  })

  const hoverExpand = delayedDragHover(800, () => {
    if (!canAcceptDirectoryPayload(LocalTransfer.peek(), args.projectId)) return
    args.onHoverExpand?.()
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeHoverExpand = hoverExpand(element)
    const disposeRegistry = DropRegistry.register({
      id: `file-tree:${args.projectId}:${args.directoryPath}`,
      element,
      accept: (payload) => canAcceptDirectoryPayload(payload, args.projectId),
      hover: () => {
        setDirectoryActive(args.projectId, args.directoryPath)
      },
      leave: () => {
        clearDirectoryActive(args.projectId, args.directoryPath)
      },
      dispatch: async (payload) => {
        clearDirectoryActive(args.projectId, args.directoryPath)
        await args.onDrop(payload)
      }
    })

    return () => {
      disposeRegistry()
      disposeHoverExpand()
      clearDirectoryActive(args.projectId, args.directoryPath)
      disposeObserver()
    }
  }
}

export function isFileTreeDropActive(projectId: string, path: string): boolean {
  return directoryStore.has(directoryKey(projectId, path))
}
