import type { Tab } from '$lib/features/workbench/model'
import { type DragPayload, stampDataTransfer } from '$lib/features/dnd/payload'
import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
import { getWindowLabel } from '$lib/features/workbench/state.svelte'

export function tabDragSource(args: { tab: Tab }) {
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
        items: [{ kind: 'tab', tabId: args.tab.id, sourceWindowLabel: getWindowLabel() }]
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
