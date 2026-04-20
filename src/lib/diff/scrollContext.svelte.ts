/**
 * Svelte context for sharing the DiffStack scroll container
 * with nested diff pane components.
 *
 * DiffStack sets the context; panes read it to drive virtualization.
 * Follows the same pattern as sidebar-state.svelte.ts.
 */

import { getContext, setContext } from 'svelte'

const DIFF_SCROLL_CTX = Symbol('diff-scroll')

export interface DiffScrollState {
  element: HTMLElement | null
  scrollTop: number
  containerHeight: number
}

export function setDiffScrollContext(state: DiffScrollState) {
  setContext(DIFF_SCROLL_CTX, state)
}

export function useDiffScroll(): DiffScrollState | undefined {
  return getContext<DiffScrollState | undefined>(DIFF_SCROLL_CTX)
}
