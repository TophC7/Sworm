/**
 * Diff viewer types and re-exports from @git-diff-view/core.
 *
 * Re-exports core types from @git-diff-view/core and provides
 * our own DiffMode enum and row discriminated unions for the
 * custom diff rendering layer.
 */

export {
  DiffFile,
  DiffFileLineType,
  DiffLineType,
  SplitSide,
  composeLen,
  getPlainDiffTemplate,
  getPlainLineTemplate,
  getSyntaxDiffTemplate,
  getSyntaxLineTemplate
} from '@git-diff-view/core'

export type {
  DiffLine,
  DiffLineItem,
  DiffHunkItem,
  DiffRange,
  SplitLineItem,
  UnifiedLineItem,
  SyntaxLine,
  SyntaxNode
} from '@git-diff-view/core'

/** Display mode for the diff viewer. */
export enum DiffMode {
  Split = 'split',
  Unified = 'unified'
}

/** A content row backed by a line index into the DiffFile. */
export interface DiffContentRow {
  kind: 'content'
  index: number
}

/** A hunk separator row with expand controls. */
export interface DiffHunkSeparatorRow {
  kind: 'hunk'
  index: number
}

/** Discriminated union for every row the virtualizer iterates over. */
export type DiffRow = DiffContentRow | DiffHunkSeparatorRow
