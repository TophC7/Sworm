import { DND_MIME, type DragPayload, stampDataTransfer } from '$lib/features/dnd/payload'
import { createHoverStore } from '$lib/features/dnd/hover-state.svelte'
import { delayedDragHover } from '$lib/features/dnd/delayed-hover'
import { dragObserver } from '$lib/features/dnd/observer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import type { FileTreeNode } from '$lib/utils/fileTree'

interface FileTreeSourceArgs {
  folderPath: string
  node: FileTreeNode<{ path: string }>
}

interface FileTreeDirectoryTargetArgs {
  folderPath: string
  directoryPath: string
  onDrop: (payload: DragPayload) => void | Promise<void>
  onHoverExpand?: () => void
}

const directoryStore = createHoverStore<true>()

function directoryKey(folderPath: string, path: string): string {
  return `${folderPath}:${path}`
}

function setDirectoryActive(folderPath: string, path: string): void {
  directoryStore.set(directoryKey(folderPath, path), true)
}

function clearDirectoryActive(folderPath: string, path: string): void {
  directoryStore.clear(directoryKey(folderPath, path))
}

function canAcceptDirectoryPayload(payload: DragPayload | null): boolean {
  if (!payload) return false
  return payload.items.some((item) => {
    if (item.kind === 'file') return true
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
            folderPath: args.folderPath
          }
        ]
      }

      LocalTransfer.set(payload)
      transfer.effectAllowed = 'move'
      stampDataTransfer(transfer, payload)
    }

    const onDragEnd = () => {
      LocalTransfer.clear()
      directoryStore.clearByPrefix(`${args.folderPath}:`)
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
  const drop = async (payload: DragPayload) => {
    clearDirectoryActive(args.folderPath, args.directoryPath)
    if (payload.items.some((item) => item.kind === 'file' && item.folderPath !== args.folderPath)) {
      notify.warning('Different folder')
      return
    }
    await args.onDrop(payload)
  }

  const observer = dragObserver({
    accept: (payload, types) => {
      if (payload) return canAcceptDirectoryPayload(payload)
      return types.includes(DND_MIME.SWORM_FILE) || types.includes(DND_MIME.FILES)
    },
    onOver: () => {
      setDirectoryActive(args.folderPath, args.directoryPath)
    },
    onLeave: () => {
      clearDirectoryActive(args.folderPath, args.directoryPath)
    },
    onDrop: (_event, payload) => drop(payload)
  })

  const hoverExpand = delayedDragHover(800, () => {
    if (!canAcceptDirectoryPayload(LocalTransfer.peek())) return
    args.onHoverExpand?.()
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeHoverExpand = hoverExpand(element)
    const disposeRegistry = DropRegistry.register({
      id: `file-tree:${args.folderPath}:${args.directoryPath}`,
      element,
      accept: canAcceptDirectoryPayload,
      hover: () => {
        setDirectoryActive(args.folderPath, args.directoryPath)
      },
      leave: () => {
        clearDirectoryActive(args.folderPath, args.directoryPath)
      },
      dispatch: drop
    })

    return () => {
      disposeRegistry()
      disposeHoverExpand()
      clearDirectoryActive(args.folderPath, args.directoryPath)
      disposeObserver()
    }
  }
}

export function isFileTreeDropActive(folderPath: string, path: string): boolean {
  return directoryStore.has(directoryKey(folderPath, path))
}
