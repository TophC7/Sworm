import type { PaneSlot, Tab } from '$lib/features/workbench/model'
import { type DragPayload, stampDataTransfer } from '$lib/features/dnd/payload'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'

interface TabDragSourceArgs {
  tab: Tab
  paneSlot: PaneSlot
  projectId: string
}

export function tabDragSource(args: TabDragSourceArgs) {
  return (element: HTMLElement) => {
    const onDragStart = (event: DragEvent) => {
      if (args.tab.locked) {
        event.preventDefault()
        return
      }

      const transfer = event.dataTransfer
      if (!transfer) {
        event.preventDefault()
        return
      }

      const payload: DragPayload = {
        source: 'internal',
        items: [
          {
            kind: 'tab',
            tabId: args.tab.id,
            projectId: args.projectId,
            sourcePaneSlot: args.paneSlot
          }
        ]
      }

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
