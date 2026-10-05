import { DND_MIME, type DragPayload } from '$lib/features/dnd/payload'
import { SvelteSet } from 'svelte/reactivity'
import { delayedDragHover } from '$lib/features/dnd/delayed-hover'
import { dragObserver } from '$lib/features/dnd/observer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'
import { dragSource, LocalTransfer } from '$lib/features/dnd/transfer.svelte'
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
  acceptOsFiles?: boolean
}

const directoryStore = new SvelteSet<string>()

function directoryKey(folderPath: string, path: string): string {
  return `${folderPath}:${path}`
}

function clearDirectoryActive(folderPath: string, path: string): void {
  directoryStore.delete(directoryKey(folderPath, path))
}

function canAcceptDirectoryPayload(payload: DragPayload | null, acceptOsFiles: boolean): boolean {
  if (!payload) return false
  return payload.items.some((item) => {
    if (item.kind === 'file') return true
    if (item.kind === 'os-files') return acceptOsFiles && item.paths.length > 0
    return false
  })
}

export function fileTreeDragSource(args: FileTreeSourceArgs) {
  return dragSource(
    () => [{
      kind: 'file',
      path: args.node.path,
      isDir: args.node.type === 'directory',
      folderPath: args.folderPath
    }],
    () => {
      const prefix = `${args.folderPath}:`
      for (const key of directoryStore) if (key.startsWith(prefix)) directoryStore.delete(key)
    }
  )
}

export function fileTreeDirectoryDropTarget(args: FileTreeDirectoryTargetArgs) {
  const acceptOsFiles = args.acceptOsFiles ?? true
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
      if (payload) return canAcceptDirectoryPayload(payload, acceptOsFiles)
      return types.includes(DND_MIME.SWORM_FILE) || (acceptOsFiles && types.includes(DND_MIME.FILES))
    },
    onOver: () => {
      directoryStore.add(directoryKey(args.folderPath, args.directoryPath))
    },
    onLeave: () => {
      clearDirectoryActive(args.folderPath, args.directoryPath)
    },
    onDrop: (_event, payload) => drop(payload)
  })

  const hoverExpand = delayedDragHover(800, () => {
    if (!canAcceptDirectoryPayload(LocalTransfer.peek(), acceptOsFiles)) return
    args.onHoverExpand?.()
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeHoverExpand = hoverExpand(element)
    const disposeRegistry = DropRegistry.register({
      id: `file-tree:${args.folderPath}:${args.directoryPath}`,
      element,
      accept: (payload) => canAcceptDirectoryPayload(payload, acceptOsFiles),
      hover: () => {
        directoryStore.add(directoryKey(args.folderPath, args.directoryPath))
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
