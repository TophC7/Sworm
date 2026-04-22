import { type DragPayload, stampDataTransfer } from '$lib/features/dnd/payload'
import { createHoverStore } from '$lib/features/dnd/hover-state.svelte'
import { dragObserver } from '$lib/features/dnd/observer.svelte'
import { DropRegistry } from '$lib/features/dnd/registry.svelte'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import type { GitChange } from '$lib/types/backend'

interface GitSourceArgs {
  projectId: string
  change: GitChange
}

interface GitDropZoneArgs {
  projectId: string
  staged: boolean
  onDropFiles: (filePaths: string[], staged: boolean) => void | Promise<void>
}

const zoneStore = createHoverStore<true>()

function zoneKey(projectId: string, staged: boolean): string {
  return `${projectId}:${staged ? 'staged' : 'unstaged'}`
}

function setZoneActive(projectId: string, staged: boolean): void {
  zoneStore.set(zoneKey(projectId, staged), true)
}

function clearZoneActive(projectId: string, staged: boolean): void {
  zoneStore.clear(zoneKey(projectId, staged))
}

function canAccept(payload: DragPayload | null, projectId: string, staged: boolean): boolean {
  if (!payload) return false
  return payload.items.some(
    (item) => item.kind === 'git-change' && item.projectId === projectId && item.staged !== staged
  )
}

function extractFiles(payload: DragPayload, projectId: string, staged: boolean): string[] {
  const files = new Set<string>()
  for (const item of payload.items) {
    if (item.kind !== 'git-change') continue
    if (item.projectId !== projectId || item.staged === staged) continue
    files.add(item.path)
  }
  return Array.from(files)
}

export function gitChangeDragSource(args: GitSourceArgs) {
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
            kind: 'git-change',
            path: args.change.path,
            staged: args.change.staged,
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
      zoneStore.clearByPrefix(`${args.projectId}:`)
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
  const observer = dragObserver({
    accept: (payload) => {
      if (payload) return canAccept(payload, args.projectId, args.staged)
      return false
    },
    onOver: () => {
      setZoneActive(args.projectId, args.staged)
    },
    onLeave: () => {
      clearZoneActive(args.projectId, args.staged)
    },
    onDrop: async (_event, payload) => {
      clearZoneActive(args.projectId, args.staged)
      const files = extractFiles(payload, args.projectId, args.staged)
      if (files.length === 0) return
      await args.onDropFiles(files, args.staged)
    }
  })

  return (element: HTMLElement) => {
    const disposeObserver = observer(element)
    const disposeRegistry = DropRegistry.register({
      id: `git-zone:${args.projectId}:${args.staged ? 'staged' : 'unstaged'}`,
      element,
      accept: (payload) => canAccept(payload, args.projectId, args.staged),
      hover: () => {
        setZoneActive(args.projectId, args.staged)
      },
      leave: () => {
        clearZoneActive(args.projectId, args.staged)
      },
      dispatch: async (payload) => {
        clearZoneActive(args.projectId, args.staged)
        const files = extractFiles(payload, args.projectId, args.staged)
        if (files.length === 0) return
        await args.onDropFiles(files, args.staged)
      }
    })

    return () => {
      disposeRegistry()
      clearZoneActive(args.projectId, args.staged)
      disposeObserver()
    }
  }
}

export function isGitDropZoneActive(projectId: string, staged: boolean): boolean {
  return zoneStore.has(zoneKey(projectId, staged))
}
