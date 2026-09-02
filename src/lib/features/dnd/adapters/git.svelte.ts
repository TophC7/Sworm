import { type DragPayload, stampDataTransfer } from '$lib/features/dnd/payload'
import { createHoverStore } from '$lib/features/dnd/hover-state.svelte'
import { dragObserver } from '$lib/features/dnd/observer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import type { GitChange } from '$lib/types/backend'

interface GitSourceArgs {
  folderPath: string
  changes: Pick<GitChange, 'path' | 'staged'>[] | (() => Pick<GitChange, 'path' | 'staged'>[])
}

interface GitDropZoneArgs {
  folderPath: string
  staged: boolean
  onDropFiles: (filePaths: string[], staged: boolean) => void | Promise<void>
}

const zoneStore = createHoverStore<true>()

function zoneKey(folderPath: string, staged: boolean): string {
  return `${folderPath}:${staged ? 'staged' : 'unstaged'}`
}

function canAccept(payload: DragPayload | null, staged: boolean): boolean {
  return payload?.items.some((item) => item.kind === 'git-change' && item.staged !== staged) ?? false
}

function extractFiles(payload: DragPayload, folderPath: string, staged: boolean): string[] {
  const files = new Set<string>()
  let crossedFolder = false
  for (const item of payload.items) {
    if (item.kind !== 'git-change' || item.staged === staged) continue
    if (item.folderPath !== folderPath) {
      crossedFolder = true
      continue
    }
    files.add(item.path)
  }
  if (crossedFolder) notify.warning('Different folder')
  return Array.from(files)
}

export function gitChangeDragSource(args: GitSourceArgs) {
  return (element: HTMLElement) => {
    const onDragStart = (event: DragEvent) => {
      const changes = typeof args.changes === 'function' ? args.changes() : args.changes
      const transfer = event.dataTransfer
      if (!transfer || changes.length === 0) {
        event.preventDefault()
        return
      }
      const payload: DragPayload = {
        source: 'internal',
        items: changes.map((change) => ({
          kind: 'git-change',
          path: change.path,
          staged: change.staged,
          folderPath: args.folderPath
        }))
      }
      LocalTransfer.set(payload)
      transfer.effectAllowed = 'move'
      stampDataTransfer(transfer, payload)
    }
    const onDragEnd = () => {
      LocalTransfer.clear()
      zoneStore.clearByPrefix(`${args.folderPath}:`)
    }
    element.addEventListener('dragstart', onDragStart)
    element.addEventListener('dragend', onDragEnd)
    return () => {
      element.removeEventListener('dragstart', onDragStart)
      element.removeEventListener('dragend', onDragEnd)
    }
  }
}

export function gitDropZone(args: GitDropZoneArgs) {
  const clear = () => zoneStore.clear(zoneKey(args.folderPath, args.staged))
  const drop = async (payload: DragPayload) => {
    clear()
    const files = extractFiles(payload, args.folderPath, args.staged)
    if (files.length > 0) await args.onDropFiles(files, args.staged)
  }
  const observer = dragObserver({
    accept: (payload) => canAccept(payload, args.staged),
    onOver: () => zoneStore.set(zoneKey(args.folderPath, args.staged), true),
    onLeave: clear,
    onDrop: (_event, payload) => drop(payload)
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeRegistry = DropRegistry.register({
      id: `git-zone:${args.folderPath}:${args.staged ? 'staged' : 'unstaged'}`,
      element,
      accept: (payload) => canAccept(payload, args.staged),
      hover: () => zoneStore.set(zoneKey(args.folderPath, args.staged), true),
      leave: clear,
      dispatch: drop
    })
    return () => {
      disposeRegistry()
      clear()
      disposeObserver()
    }
  }
}

export function isGitDropZoneActive(folderPath: string, staged: boolean): boolean {
  return zoneStore.has(zoneKey(folderPath, staged))
}
