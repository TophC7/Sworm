/**
 * Per-pane row virtualizer driven by an external scroll container.
 *
 * Each diff pane (unified or split) creates its own virtualizer.
 * The virtualizer reads the DiffStack scroll container's position
 * from context and uses getBoundingClientRect() to compute which
 * rows fall within the viewport, then returns spacer heights and
 * a visible row slice.
 *
 * Row height is fixed (fontSize * 1.6) — wrap mode uses this as
 * an estimate and accepts minor inaccuracy for simplicity.
 */

import type { DiffScrollState } from './scrollContext.svelte'
import type { DiffRow } from './types'

/** Virtualizer output — consumed by pane templates. */
export interface PaneVirtualizerState {
  /** Whether virtualization is active (false for small diffs). */
  readonly enabled: boolean
  /** Height of spacer before visible rows (px). */
  readonly spacerBefore: number
  /** Height of spacer after visible rows (px). */
  readonly spacerAfter: number
  /** Total height of all rows (px). */
  readonly totalHeight: number
  /** Slice of rows to render. */
  readonly visibleRows: DiffRow[]
  /** Index of first visible row in the full rows array (for #each keying). */
  readonly startIndex: number
}

const INITIAL_RENDER_COUNT = 50

/**
 * Create a reactive virtualizer for a diff pane.
 *
 * All inputs are getter functions so the virtualizer re-derives
 * automatically when rows, font size, or scroll position change.
 */
export function createPaneVirtualizer(config: {
  /** Source rows from buildRows(). */
  getRows: () => DiffRow[]
  /** Row height in px (typically fontSize * 1.6). */
  getRowHeight: () => number
  /** Scroll context from DiffStack. */
  getScrollCtx: () => DiffScrollState
  /** The pane's own root element. */
  getPaneEl: () => HTMLElement | null
  /** Extra rows above/below viewport. */
  overscan?: number
  /** Minimum row count to activate virtualization. */
  threshold?: number
}): PaneVirtualizerState {
  const overscan = config.overscan ?? 20
  const threshold = config.threshold ?? 100

  const derived: PaneVirtualizerState = $derived.by(() => {
    const rows = config.getRows()
    const rowHeight = config.getRowHeight()
    const totalHeight = rows.length * rowHeight

    // Small diffs: render everything, no spacers
    if (rows.length < threshold) {
      return {
        enabled: false,
        spacerBefore: 0,
        spacerAfter: 0,
        totalHeight,
        visibleRows: rows,
        startIndex: 0
      }
    }

    const ctx = config.getScrollCtx()
    const paneEl = config.getPaneEl()

    // Before measurement is available, render the first batch
    if (!paneEl || !ctx.element || ctx.containerHeight === 0) {
      const count = Math.min(rows.length, INITIAL_RENDER_COUNT + overscan)
      return {
        enabled: true,
        spacerBefore: 0,
        spacerAfter: Math.max(0, (rows.length - count) * rowHeight),
        totalHeight,
        visibleRows: rows.slice(0, count),
        startIndex: 0
      }
    }

    // Read scroll context (reactive dependency on scrollTop/containerHeight)
    const { containerHeight } = ctx
    // Force reactive dependency on scrollTop so we re-derive on scroll
    void ctx.scrollTop

    // How far the pane is from the scroll container's viewport top
    const containerRect = ctx.element.getBoundingClientRect()
    const paneRect = paneEl.getBoundingClientRect()
    const paneOffset = paneRect.top - containerRect.top

    // Visible region of the pane within the scroll viewport
    // paneOffset < 0 means the top of the pane is above the viewport
    const visibleStart = Math.max(0, -paneOffset)
    const visibleEnd = Math.min(totalHeight, -paneOffset + containerHeight)

    // If pane is entirely off-screen, render nothing
    if (visibleEnd <= 0 || visibleStart >= totalHeight) {
      return {
        enabled: true,
        spacerBefore: 0,
        spacerAfter: totalHeight,
        totalHeight,
        visibleRows: [],
        startIndex: 0
      }
    }

    // Compute row range with overscan
    const startIndex = Math.max(0, Math.floor(visibleStart / rowHeight) - overscan)
    const endIndex = Math.min(rows.length - 1, Math.ceil(visibleEnd / rowHeight) + overscan)

    return {
      enabled: true,
      spacerBefore: startIndex * rowHeight,
      spacerAfter: Math.max(0, (rows.length - 1 - endIndex) * rowHeight),
      totalHeight,
      visibleRows: rows.slice(startIndex, endIndex + 1),
      startIndex
    }
  })

  // Return getters so consumers read through the $derived reactively
  return {
    get enabled() {
      return derived.enabled
    },
    get spacerBefore() {
      return derived.spacerBefore
    },
    get spacerAfter() {
      return derived.spacerAfter
    },
    get totalHeight() {
      return derived.totalHeight
    },
    get visibleRows() {
      return derived.visibleRows
    },
    get startIndex() {
      return derived.startIndex
    }
  }
}
