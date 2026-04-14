/**
 * Reactive viewmodel that converts a DiffFile into flat row arrays
 * suitable for virtual scrolling.
 *
 * Key insight from the core library: a given index can have BOTH a
 * hunk separator AND content. Hunks are detected via getSplitHunkLine/
 * getUnifiedHunkLine (not via diff.type). The viewmodel emits hunk
 * rows BEFORE their associated content row, plus a trailing hunk
 * at the end of the file.
 */

import { DiffLineType, type DiffFile, type DiffRow } from '$lib/diff/types'

/**
 * Build a flat DiffRow[] for the given mode.
 *
 * Intended to be called inside a `$derived.by()` so it re-evaluates
 * when the DiffFile notifies subscribers (e.g. after hunk expansion).
 */
export function buildRows(diffFile: DiffFile, mode: 'split' | 'unified'): DiffRow[] {
  const rows: DiffRow[] = []
  const length = mode === 'split' ? diffFile.splitLineLength : diffFile.unifiedLineLength

  for (let i = 0; i < length; i++) {
    // Check for hunk at this index (independent of content)
    const hunk = mode === 'split' ? diffFile.getSplitHunkLine(i) : diffFile.getUnifiedHunkLine(i)

    if (hunk) {
      rows.push({ kind: 'hunk', index: i })
    }

    // Check for content at this index
    if (mode === 'split') {
      const left = diffFile.getSplitLeftLine(i)
      const right = diffFile.getSplitRightLine(i)
      if (left?.isHidden && right?.isHidden) continue
      if ((left?.isHidden && !right?.lineNumber) || (right?.isHidden && !left?.lineNumber)) continue
      // Skip if this is ONLY a hunk with no actual content
      if (!left?.lineNumber && !right?.lineNumber && hunk) continue
      rows.push({ kind: 'content', index: i })
    } else {
      const line = diffFile.getUnifiedLine(i)
      if (!line || line.isHidden) continue
      if (!line.oldLineNumber && !line.newLineNumber && hunk) continue
      rows.push({ kind: 'content', index: i })
    }
  }

  // Trailing hunk at the end (for expanding beyond the last line)
  const trailingHunk = mode === 'split' ? diffFile.getSplitHunkLine(length) : diffFile.getUnifiedHunkLine(length)
  if (trailingHunk) {
    rows.push({ kind: 'hunk', index: length })
  }

  return rows
}
