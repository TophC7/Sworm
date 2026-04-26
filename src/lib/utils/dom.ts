// Walk up from `el` to find the nearest scrollable ancestor (a
// container whose `overflow-y` is `auto` or `scroll`). Returns null
// when no such ancestor exists. Used by virtualizing primitives that
// need to track scroll offsets relative to a viewport without prop-
// drilling the scroll element through every layer.
export function findScrollParent(el: HTMLElement | null): HTMLElement | null {
  let current = el?.parentElement ?? null
  while (current) {
    const overflowY = getComputedStyle(current).overflowY
    if (overflowY === 'auto' || overflowY === 'scroll') {
      return current
    }
    current = current.parentElement
  }
  return null
}
