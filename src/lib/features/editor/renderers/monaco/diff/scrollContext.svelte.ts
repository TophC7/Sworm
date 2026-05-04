/**
 * Svelte context that hands the DiffStack scroll container down to nested
 * diff panes. The IntersectionObserver inside `MonacoDiffBody` needs the
 * scroll element as its root; nothing else in the diff renderer reads
 * scroll metrics.
 *
 * The state is kept intentionally minimal: only `element`. Earlier
 * versions exposed `scrollTop` and `containerHeight` and updated them on
 * every scroll frame, which created a reactive surface that any future
 * `$effect` reading those properties would re-run on every frame. Since
 * no consumer needed them, they were removed; an IntersectionObserver
 * alone is sufficient for "is this row near the viewport".
 *
 * Mirrors the pattern in sidebar-state.svelte.ts.
 */

import { getContext, setContext } from 'svelte'

const DIFF_SCROLL_CTX = Symbol('diff-scroll')

export interface DiffScrollState {
  /**
   * The scroll container element owned by `DiffStack`. `null` until
   * `bind:this` resolves on first paint, then stable for the lifetime
   * of the stack.
   */
  element: HTMLElement | null
}

export function setDiffScrollContext(state: DiffScrollState) {
  setContext(DIFF_SCROLL_CTX, state)
}

export function useDiffScroll(): DiffScrollState | undefined {
  return getContext<DiffScrollState | undefined>(DIFF_SCROLL_CTX)
}
