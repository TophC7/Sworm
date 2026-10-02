import type { Tab } from '$lib/features/workbench/model'
import { type DragPayload, type SwormDragKind, stampDataTransfer } from '$lib/features/dnd/payload'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import { getWorkbenchId } from '$lib/features/workbench/state.svelte'

/** Drag source for one title-bar item; `item` returns null to refuse the drag. */
function stripDragSource(item: () => SwormDragKind | null) {
  return (element: HTMLElement) => {
    const onDragStart = (event: DragEvent) => {
      const dragged = item()
      const transfer = event.dataTransfer
      if (!dragged || !transfer) {
        event.preventDefault()
        return
      }

      const payload: DragPayload = { source: 'internal', items: [dragged] }
      LocalTransfer.set(payload)
      transfer.effectAllowed = 'move'
      stampDataTransfer(transfer, payload)
    }

    const onDragEnd = () => {
      LocalTransfer.clear()
    }

    element.addEventListener('dragstart', onDragStart)
    element.addEventListener('dragend', onDragEnd)

    return () => {
      element.removeEventListener('dragstart', onDragStart)
      element.removeEventListener('dragend', onDragEnd)
    }
  }
}

export function tabDragSource(args: { tab: Tab }) {
  return stripDragSource(() =>
    args.tab.locked ? null : { kind: 'tab', tabId: args.tab.id, sourceWindowLabel: getWorkbenchId() }
  )
}

/** The server tab drags its whole group. */
export function groupDragSource(args: { server: string; workbenchId: string }) {
  return stripDragSource(() => ({
    kind: 'workbench',
    server: args.server,
    workbenchId: args.workbenchId,
    sourceWindowLabel: getWorkbenchId()
  }))
}
