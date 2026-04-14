/**
 * Shared layout constants and utilities for diff rendering.
 *
 * Single source of truth for line-height ratio and gutter sizing
 * used across unified, split, and hunk row components.
 */

/** Line-height multiplier applied to fontSize for row height. */
export const LINE_HEIGHT_RATIO = 1.6

/** Compute row height in px from font size. */
export function lineHeight(fontSize: number): string {
  return `${fontSize * LINE_HEIGHT_RATIO}px`
}

/**
 * Compute gutter width based on the maximum line number.
 * Returns a CSS em value that accommodates the digit count.
 */
export function gutterWidth(maxLineNumber: number): string {
  const digits = String(Math.max(maxLineNumber, 1)).length
  return `${Math.max(digits * 0.6 + 1.2, 2.5)}em`
}
