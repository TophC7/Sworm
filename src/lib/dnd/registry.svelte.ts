import type { DragPayload } from '$lib/dnd/payload'

interface TargetEntry {
  id: string
  element: HTMLElement
  accept: (payload: DragPayload) => boolean
  hitTest?: (payload: DragPayload, clientX: number, clientY: number) => boolean
  hover?: (payload: DragPayload, clientX: number, clientY: number) => void
  leave?: () => void
  dispatch: (payload: DragPayload, clientX: number, clientY: number) => void | Promise<void>
}

const targets = new Map<string, TargetEntry>()

let hoveredTargetId: string | null = null

function ancestorDistance(node: Element, ancestor: HTMLElement): number {
  let distance = 0
  let cursor: Node | null = node
  while (cursor) {
    if (cursor === ancestor) return distance
    cursor = cursor.parentNode
    distance += 1
  }
  return Number.POSITIVE_INFINITY
}

function pickDeepestTarget(node: Element, candidates: TargetEntry[]): TargetEntry | null {
  if (candidates.length === 0) return null
  let best = candidates[0]
  let bestDistance = ancestorDistance(node, best.element)
  for (const candidate of candidates.slice(1)) {
    const distance = ancestorDistance(node, candidate.element)
    if (distance < bestDistance) {
      best = candidate
      bestDistance = distance
    }
  }
  return Number.isFinite(bestDistance) ? best : null
}

function switchHoverTarget(next: TargetEntry | null): void {
  if (hoveredTargetId && hoveredTargetId !== next?.id) {
    targets.get(hoveredTargetId)?.leave?.()
  }

  if (!next) {
    hoveredTargetId = null
    return
  }

  hoveredTargetId = next.id
}

export const DropRegistry = {
  register(target: TargetEntry): () => void {
    targets.set(target.id, target)
    return () => {
      if (hoveredTargetId === target.id) {
        target.leave?.()
        hoveredTargetId = null
      }
      targets.delete(target.id)
    }
  },

  findAt(clientX: number, clientY: number, payload: DragPayload): TargetEntry | null {
    const stack = document.elementsFromPoint(clientX, clientY)
    const entries = Array.from(targets.values()).filter(
      (entry) => entry.accept(payload) && (entry.hitTest?.(payload, clientX, clientY) ?? true)
    )
    if (entries.length === 0) return null

    for (const node of stack) {
      const candidates = entries.filter((entry) => entry.element === node || entry.element.contains(node))
      const best = pickDeepestTarget(node, candidates)
      if (best) return best
    }

    return null
  },

  async dispatchAt(clientX: number, clientY: number, payload: DragPayload): Promise<boolean> {
    const target = this.findAt(clientX, clientY, payload)
    if (!target) return false
    switchHoverTarget(target)
    await target.dispatch(payload, clientX, clientY)
    target.leave?.()
    hoveredTargetId = null
    return true
  },

  hoverAt(clientX: number, clientY: number, payload: DragPayload): void {
    const target = this.findAt(clientX, clientY, payload)
    switchHoverTarget(target)
    target?.hover?.(payload, clientX, clientY)
  },

  clearHover(): void {
    if (!hoveredTargetId) return
    targets.get(hoveredTargetId)?.leave?.()
    hoveredTargetId = null
  }
}
