import { DND_MIME, type DragPayload, dragTypes, parsePayload } from '$lib/dnd/payload'
import { LocalTransfer } from '$lib/dnd/transfer.svelte'

export interface DragFrame {
  clientX: number
  clientY: number
  localX: number
  localY: number
  width: number
  height: number
  rect: DOMRectReadOnly
}

/**
 * Build a {@link DragFrame} from a client-space point against an
 * element. Used by registry callbacks (which receive raw coordinates
 * from `DropRegistry.dispatchAt`) so they can reuse the same zone /
 * hit-test math the observer's callbacks use.
 *
 * Returns `null` for degenerate rects (hidden / zero-size element).
 */
export function frameAt(element: HTMLElement, clientX: number, clientY: number): DragFrame | null {
  const rect = element.getBoundingClientRect()
  if (rect.width <= 0 || rect.height <= 0) return null
  return {
    clientX,
    clientY,
    localX: clientX - rect.left,
    localY: clientY - rect.top,
    width: rect.width,
    height: rect.height,
    rect
  }
}

interface DragObserverOptions {
  accept: (payload: DragPayload | null, types: readonly string[]) => boolean
  onEnter?: (event: DragEvent, frame: DragFrame) => void
  onOver?: (event: DragEvent, frame: DragFrame) => void
  onLeave?: () => void
  onDrop?: (event: DragEvent, payload: DragPayload, frame: DragFrame | null) => void | Promise<void>
  dropEffect?: DataTransfer['dropEffect']
}

export function dragObserver(options: DragObserverOptions) {
  return (element: HTMLElement) => {
    let counter = 0
    let rectCache: DOMRectReadOnly | null = null
    let rafPending = false
    let lastEvent: DragEvent | null = null

    const clearActiveState = () => {
      counter = 0
      rectCache = null
      rafPending = false
      lastEvent = null
    }

    const frameFrom = (event: DragEvent): DragFrame => {
      if (!rectCache) {
        rectCache = element.getBoundingClientRect()
      }
      return {
        clientX: event.clientX,
        clientY: event.clientY,
        localX: event.clientX - rectCache.left,
        localY: event.clientY - rectCache.top,
        width: rectCache.width,
        height: rectCache.height,
        rect: rectCache
      }
    }

    const beginHoverFrom = (event: DragEvent) => {
      rectCache = element.getBoundingClientRect()
      options.onEnter?.(event, frameFrom(event))
    }

    const onDragEnter = (event: DragEvent) => {
      const payload = LocalTransfer.peek()
      const types = dragTypes(event)
      if (!options.accept(payload, types)) return
      event.preventDefault()
      counter += 1
      if (counter !== 1) return
      beginHoverFrom(event)
    }

    const onDragOver = (event: DragEvent) => {
      if (counter === 0) {
        // Fallback initializer. Some webviews (WebKitGTK in particular)
        // skip dragenter on the observed element when the drag starts
        // inside it — the source element gets the initial enter but the
        // ancestor observer never sees it. Lazily begin hover from the
        // first dragover so same-pane tab drags still register.
        const payload = LocalTransfer.peek()
        const types = dragTypes(event)
        if (!options.accept(payload, types)) return
        counter = 1
        beginHoverFrom(event)
      }
      event.preventDefault()
      if (event.dataTransfer) {
        event.dataTransfer.dropEffect = options.dropEffect ?? 'move'
      }
      lastEvent = event
      if (rafPending) return
      rafPending = true
      requestAnimationFrame(() => {
        rafPending = false
        const pending = lastEvent
        if (!pending || counter === 0) return
        options.onOver?.(pending, frameFrom(pending))
      })
    }

    const onDragLeave = () => {
      if (counter === 0) return
      counter -= 1
      if (counter > 0) return
      clearActiveState()
      options.onLeave?.()
    }

    const onDrop = async (event: DragEvent) => {
      event.preventDefault()
      const frame = rectCache ? frameFrom(event) : null
      clearActiveState()
      options.onLeave?.()
      const payload = LocalTransfer.peek() ?? parsePayload(event.dataTransfer?.getData(DND_MIME.SWORM_ITEM))
      if (!payload) return
      await options.onDrop?.(event, payload, frame)
    }

    const onGlobalDragEnd = () => {
      // Fires on the source element (bubbles to window) whenever a drag
      // terminates — including ESC cancel or release outside any drop
      // target. Without this, a drag that never fires dragleave on the
      // observed element would leave stale counter/rect state behind.
      if (counter === 0) return
      clearActiveState()
      options.onLeave?.()
    }

    element.addEventListener('dragenter', onDragEnter)
    element.addEventListener('dragover', onDragOver)
    element.addEventListener('dragleave', onDragLeave)
    element.addEventListener('drop', onDrop)
    window.addEventListener('dragend', onGlobalDragEnd)

    return () => {
      clearActiveState()
      element.removeEventListener('dragenter', onDragEnter)
      element.removeEventListener('dragover', onDragOver)
      element.removeEventListener('dragleave', onDragLeave)
      element.removeEventListener('drop', onDrop)
      window.removeEventListener('dragend', onGlobalDragEnd)
    }
  }
}
