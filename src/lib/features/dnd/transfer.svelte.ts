import { stampDataTransfer, type DragPayload, type SwormDragKind } from '$lib/features/dnd/payload'

let currentPayload = $state<DragPayload | null>(null)

// Window listeners are attached only while a payload is set so we don't
// react to unrelated drags (file drops from outside, future DnD systems
// in the same document). Re-attached on every `set()` and torn down as
// soon as the payload clears.
let safetyNetAttached = false

function detachSafetyNet(): void {
  if (!safetyNetAttached) return
  window.removeEventListener('dragend', forceClear)
  window.removeEventListener('drop', forceClear)
  safetyNetAttached = false
}

function forceClear(): void {
  currentPayload = null
  detachSafetyNet()
}

function attachSafetyNet(): void {
  if (safetyNetAttached || typeof window === 'undefined') return
  window.addEventListener('dragend', forceClear)
  window.addEventListener('drop', forceClear)
  safetyNetAttached = true
}

export const LocalTransfer = {
  set(payload: DragPayload): void {
    currentPayload = payload
    // Safety net for dangling state. Per-source adapters already clear
    // on `dragend`, but WebKitGTK occasionally fails to emit it (source
    // element removed mid-drag, ESC cancel, drag leaves the window). A
    // stuck payload keeps drop targets armed and blocks subsequent drags
    // until another drag finally resets the state.
    attachSafetyNet()
  },
  clear(): void {
    currentPayload = null
    detachSafetyNet()
  },
  peek(): DragPayload | null {
    return currentPayload
  },
  has(kind: SwormDragKind['kind']): boolean {
    return currentPayload?.items.some((item) => item.kind === kind) ?? false
  }
}

/** Shared source lifecycle; null or empty items refuse the drag. */
export function dragSource(items: () => SwormDragKind[] | null, onEnd?: () => void) {
  return (element: HTMLElement) => {
    const onDragStart = (event: DragEvent) => {
      const dragged = items()
      const transfer = event.dataTransfer
      if (!transfer || !dragged?.length) {
        event.preventDefault()
        return
      }

      const payload: DragPayload = { source: 'internal', items: dragged }
      LocalTransfer.set(payload)
      transfer.effectAllowed = 'move'
      stampDataTransfer(transfer, payload)
    }
    const onDragEnd = () => {
      LocalTransfer.clear()
      onEnd?.()
    }

    element.addEventListener('dragstart', onDragStart)
    element.addEventListener('dragend', onDragEnd)
    return () => {
      element.removeEventListener('dragstart', onDragStart)
      element.removeEventListener('dragend', onDragEnd)
    }
  }
}
