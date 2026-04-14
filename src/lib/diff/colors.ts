/**
 * Maps DiffLineType values to Tailwind utility classes
 * using the existing Sworm design tokens.
 */

import { DiffLineType } from './types'

/** Background class for line content area. */
export function lineBgClass(type: DiffLineType): string {
  switch (type) {
    case DiffLineType.Add:
      return 'bg-success/8'
    case DiffLineType.Delete:
      return 'bg-danger/8'
    case DiffLineType.Hunk:
      return 'bg-accent/6'
    default:
      return 'bg-ground'
  }
}

/** Background class for gutter (line number) cells. */
export function gutterBgClass(type: DiffLineType): string {
  switch (type) {
    case DiffLineType.Add:
      return 'bg-success/12'
    case DiffLineType.Delete:
      return 'bg-danger/12'
    case DiffLineType.Hunk:
      return 'bg-accent/10'
    default:
      return 'bg-surface'
  }
}
