import type { DragPayload, SwormDragKind } from '$lib/features/dnd/payload'

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
    // stuck payload keeps the pane content shield mounted, which eats
    // subsequent clicks until another drag finally resets the state.
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
