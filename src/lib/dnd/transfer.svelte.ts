import type { DragPayload, SwormDragKind } from '$lib/dnd/payload'

let currentPayload = $state<DragPayload | null>(null)

export const LocalTransfer = {
  set(payload: DragPayload): void {
    currentPayload = payload
  },
  clear(): void {
    currentPayload = null
  },
  peek(): DragPayload | null {
    return currentPayload
  },
  has(kind: SwormDragKind['kind']): boolean {
    return currentPayload?.items.some((item) => item.kind === kind) ?? false
  }
}
