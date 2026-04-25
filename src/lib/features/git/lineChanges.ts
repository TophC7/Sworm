export interface LineChange {
  readonly originalStartLineNumber: number
  readonly originalEndLineNumber: number
  readonly modifiedStartLineNumber: number
  readonly modifiedEndLineNumber: number
}

export interface LineSelectionRange {
  readonly startLineNumber: number
  readonly endLineNumber: number
}

export interface LineChangeContent {
  readonly originalContent: string
  readonly modifiedContent: string
}

interface TextLine {
  start: number
  end: number
}

class TextSnapshot {
  readonly lines: TextLine[]

  constructor(readonly content: string) {
    this.lines = parseLines(content)
  }

  get lineCount(): number {
    return this.lines.length
  }

  lineLength(line: number): number {
    const entry = this.lines[Math.min(Math.max(line, 0), this.lines.length - 1)]
    return Math.max(0, entry.end - entry.start)
  }

  lineContent(lineNumber: number): string {
    if (lineNumber < 1 || lineNumber > this.lines.length) return ''
    const entry = this.lines[lineNumber - 1]
    return this.content.slice(entry.start, entry.end)
  }

  offsetAt(line: number, character: number): number {
    if (line <= 0) return Math.max(0, character)
    if (line >= this.lines.length) return this.content.length
    const entry = this.lines[line]
    return Math.min(entry.end, entry.start + Math.max(0, character))
  }

  textInRange(startLine: number, startCharacter: number, endLine: number, endCharacter: number): string {
    const start = this.offsetAt(startLine, startCharacter)
    const end = this.offsetAt(endLine, endCharacter)
    return this.content.slice(start, end)
  }
}

function parseLines(content: string): TextLine[] {
  if (content.length === 0) return [{ start: 0, end: 0 }]

  const lines: TextLine[] = []
  let start = 0
  const eol = /\r\n|\r|\n/g
  let match: RegExpExecArray | null
  while ((match = eol.exec(content))) {
    lines.push({ start, end: match.index })
    start = match.index + match[0].length
  }
  lines.push({ start, end: content.length })
  return lines
}

export function compareLineChanges(a: LineChange, b: LineChange): number {
  return (
    a.modifiedStartLineNumber - b.modifiedStartLineNumber ||
    a.modifiedEndLineNumber - b.modifiedEndLineNumber ||
    a.originalStartLineNumber - b.originalStartLineNumber ||
    a.originalEndLineNumber - b.originalEndLineNumber
  )
}

export function invertLineChange(change: LineChange): LineChange {
  return {
    originalStartLineNumber: change.modifiedStartLineNumber,
    originalEndLineNumber: change.modifiedEndLineNumber,
    modifiedStartLineNumber: change.originalStartLineNumber,
    modifiedEndLineNumber: change.originalEndLineNumber
  }
}

export function applyLineChanges(
  originalContent: string,
  modifiedContent: string,
  changesToApply: readonly LineChange[]
): string {
  const changes = [...changesToApply].sort(compareLineChanges)
  if (changes.length === 0) return originalContent

  const original = new TextSnapshot(originalContent)
  const modified = new TextSnapshot(modifiedContent)
  const result: string[] = []
  let currentLine = 0

  for (const change of changes) {
    const isInsertion = change.originalEndLineNumber === 0
    const isDeletion = change.modifiedEndLineNumber === 0

    let endLine = isInsertion ? change.originalStartLineNumber : change.originalStartLineNumber - 1
    let endCharacter = 0

    if (isDeletion && change.originalEndLineNumber === original.lineCount) {
      endLine -= 1
      endCharacter = endLine < 0 ? 0 : original.lineLength(endLine)
      endLine = Math.max(0, endLine)
    }

    result.push(original.textInRange(currentLine, 0, endLine, endCharacter))

    if (!isDeletion) {
      let fromLine = change.modifiedStartLineNumber - 1
      let fromCharacter = 0

      if (isInsertion && change.originalStartLineNumber === original.lineCount) {
        fromLine -= 1
        fromCharacter = fromLine < 0 ? 0 : modified.lineLength(fromLine)
        fromLine = Math.max(0, fromLine)
      }

      result.push(modified.textInRange(fromLine, fromCharacter, change.modifiedEndLineNumber, 0))
    }

    currentLine = isInsertion ? change.originalStartLineNumber : change.originalEndLineNumber
  }

  result.push(original.textInRange(currentLine, 0, original.lineCount, 0))
  return result.join('')
}

export function toLineSelectionRanges(
  selections: readonly {
    startLineNumber: number
    endLineNumber: number
  }[]
): LineSelectionRange[] {
  const sorted = selections
    .map((selection) => ({
      startLineNumber: Math.min(selection.startLineNumber, selection.endLineNumber),
      endLineNumber: Math.max(selection.startLineNumber, selection.endLineNumber)
    }))
    .sort((a, b) => a.startLineNumber - b.startLineNumber || a.endLineNumber - b.endLineNumber)

  const result: LineSelectionRange[] = []
  for (const range of sorted) {
    const last = result[result.length - 1]
    if (!last || range.startLineNumber > last.endLineNumber + 1) {
      result.push(range)
      continue
    }
    result[result.length - 1] = {
      startLineNumber: last.startLineNumber,
      endLineNumber: Math.max(last.endLineNumber, range.endLineNumber)
    }
  }
  return result
}

export function modifiedRangeForLineChange(change: LineChange, modifiedLineCount: number): LineSelectionRange {
  if (change.modifiedEndLineNumber === 0) {
    const anchor = Math.min(Math.max(1, change.modifiedStartLineNumber || 1), Math.max(1, modifiedLineCount))
    return { startLineNumber: anchor, endLineNumber: anchor }
  }

  return {
    startLineNumber: Math.max(1, change.modifiedStartLineNumber),
    endLineNumber: Math.max(change.modifiedStartLineNumber, change.modifiedEndLineNumber)
  }
}

function rangesIntersect(a: LineSelectionRange, b: LineSelectionRange): boolean {
  return a.startLineNumber <= b.endLineNumber && b.startLineNumber <= a.endLineNumber
}

function rangeContains(a: LineSelectionRange, b: LineSelectionRange): boolean {
  return a.startLineNumber <= b.startLineNumber && b.endLineNumber <= a.endLineNumber
}

function rangeLineCount(startLineNumber: number, endLineNumber: number): number {
  if (endLineNumber === 0) return 0
  return Math.max(0, endLineNumber - startLineNumber + 1)
}

function hasSameLineCount(change: LineChange): boolean {
  return (
    rangeLineCount(change.originalStartLineNumber, change.originalEndLineNumber) ===
    rangeLineCount(change.modifiedStartLineNumber, change.modifiedEndLineNumber)
  )
}

function canSliceByModifiedRange(change: LineChange): boolean {
  return change.originalEndLineNumber === 0 || hasSameLineCount(change)
}

function subtractRanges(range: LineSelectionRange, subtract: readonly LineSelectionRange[]): LineSelectionRange[] {
  let remaining: LineSelectionRange[] = [range]

  for (const cut of subtract) {
    const next: LineSelectionRange[] = []
    for (const segment of remaining) {
      if (!rangesIntersect(segment, cut)) {
        next.push(segment)
        continue
      }

      if (segment.startLineNumber < cut.startLineNumber) {
        next.push({
          startLineNumber: segment.startLineNumber,
          endLineNumber: cut.startLineNumber - 1
        })
      }
      if (cut.endLineNumber < segment.endLineNumber) {
        next.push({
          startLineNumber: cut.endLineNumber + 1,
          endLineNumber: segment.endLineNumber
        })
      }
    }
    remaining = next
  }

  return remaining
}

const MAX_REFINED_LINE_CHANGE_CELLS = 40_000

// Monaco can report a mixed-size hunk as one coarse replacement. Refining that
// hunk into stable line-level pieces lets selection actions keep neighbors intact.
function lcsLinePairs(originalLines: readonly string[], modifiedLines: readonly string[]): Array<[number, number]> {
  const rows = originalLines.length
  const columns = modifiedLines.length
  const matrix: number[][] = Array.from({ length: rows + 1 }, () => Array(columns + 1).fill(0))

  for (let row = rows - 1; row >= 0; row -= 1) {
    for (let column = columns - 1; column >= 0; column -= 1) {
      matrix[row][column] =
        originalLines[row] === modifiedLines[column]
          ? matrix[row + 1][column + 1] + 1
          : Math.max(matrix[row + 1][column], matrix[row][column + 1])
    }
  }

  const pairs: Array<[number, number]> = []
  let row = 0
  let column = 0
  while (row < rows && column < columns) {
    if (originalLines[row] === modifiedLines[column]) {
      pairs.push([row, column])
      row += 1
      column += 1
    } else if (matrix[row + 1][column] >= matrix[row][column + 1]) {
      row += 1
    } else {
      column += 1
    }
  }

  return pairs
}

function insertionAnchor(change: LineChange, originalOffset: number): number {
  return change.originalStartLineNumber + originalOffset - 1
}

function deletionAnchor(change: LineChange, modifiedOffset: number): number {
  return change.modifiedStartLineNumber + modifiedOffset - 1
}

function appendGapLineChanges(
  result: LineChange[],
  change: LineChange,
  originalStartOffset: number,
  originalEndOffset: number,
  modifiedStartOffset: number,
  modifiedEndOffset: number
): void {
  const originalGapLineCount = Math.max(0, originalEndOffset - originalStartOffset + 1)
  const modifiedGapLineCount = Math.max(0, modifiedEndOffset - modifiedStartOffset + 1)
  const pairedLineCount = Math.min(originalGapLineCount, modifiedGapLineCount)

  if (pairedLineCount > 0) {
    result.push({
      originalStartLineNumber: change.originalStartLineNumber + originalStartOffset,
      originalEndLineNumber: change.originalStartLineNumber + originalStartOffset + pairedLineCount - 1,
      modifiedStartLineNumber: change.modifiedStartLineNumber + modifiedStartOffset,
      modifiedEndLineNumber: change.modifiedStartLineNumber + modifiedStartOffset + pairedLineCount - 1
    })
  }

  if (originalGapLineCount > pairedLineCount) {
    result.push({
      originalStartLineNumber: change.originalStartLineNumber + originalStartOffset + pairedLineCount,
      originalEndLineNumber: change.originalStartLineNumber + originalEndOffset,
      modifiedStartLineNumber: deletionAnchor(change, modifiedStartOffset + pairedLineCount),
      modifiedEndLineNumber: 0
    })
  }

  if (modifiedGapLineCount > pairedLineCount) {
    result.push({
      originalStartLineNumber: insertionAnchor(change, originalStartOffset + pairedLineCount),
      originalEndLineNumber: 0,
      modifiedStartLineNumber: change.modifiedStartLineNumber + modifiedStartOffset + pairedLineCount,
      modifiedEndLineNumber: change.modifiedStartLineNumber + modifiedEndOffset
    })
  }
}

function splitLineChangeByContent(change: LineChange, original: TextSnapshot, modified: TextSnapshot): LineChange[] {
  const originalLineCount = rangeLineCount(change.originalStartLineNumber, change.originalEndLineNumber)
  const modifiedLineCount = rangeLineCount(change.modifiedStartLineNumber, change.modifiedEndLineNumber)

  if (originalLineCount === 0 || modifiedLineCount === 0) return [change]
  if (originalLineCount * modifiedLineCount > MAX_REFINED_LINE_CHANGE_CELLS) return [change]

  const originalLines = Array.from({ length: originalLineCount }, (_, index) =>
    original.lineContent(change.originalStartLineNumber + index)
  )
  const modifiedLines = Array.from({ length: modifiedLineCount }, (_, index) =>
    modified.lineContent(change.modifiedStartLineNumber + index)
  )
  const pairs = lcsLinePairs(originalLines, modifiedLines)
  const result: LineChange[] = []
  let previousOriginal = -1
  let previousModified = -1

  for (const [nextOriginal, nextModified] of [...pairs, [originalLineCount, modifiedLineCount] as [number, number]]) {
    const originalStartOffset = previousOriginal + 1
    const originalEndOffset = nextOriginal - 1
    const modifiedStartOffset = previousModified + 1
    const modifiedEndOffset = nextModified - 1
    appendGapLineChanges(result, change, originalStartOffset, originalEndOffset, modifiedStartOffset, modifiedEndOffset)

    previousOriginal = nextOriginal
    previousModified = nextModified
  }

  return result.length > 0 ? result : []
}

function refineLineChanges(changes: readonly LineChange[], content?: LineChangeContent): LineChange[] {
  if (!content) return [...changes].sort(compareLineChanges)

  const original = new TextSnapshot(content.originalContent)
  const modified = new TextSnapshot(content.modifiedContent)
  return changes.flatMap((change) => splitLineChangeByContent(change, original, modified)).sort(compareLineChanges)
}

export function lineChangeIntersectsRanges(
  change: LineChange,
  ranges: readonly LineSelectionRange[],
  modifiedLineCount: number
): boolean {
  const modifiedRange = modifiedRangeForLineChange(change, modifiedLineCount)
  return ranges.some((range) => rangesIntersect(modifiedRange, range))
}

export function intersectLineChangeWithRange(
  change: LineChange,
  range: LineSelectionRange,
  modifiedLineCount: number
): LineChange | null {
  const modifiedRange = modifiedRangeForLineChange(change, modifiedLineCount)
  if (!rangesIntersect(modifiedRange, range)) return null

  if (change.modifiedEndLineNumber === 0) {
    return change
  }

  if (change.originalEndLineNumber === 0) {
    return {
      originalStartLineNumber: change.originalStartLineNumber,
      originalEndLineNumber: 0,
      modifiedStartLineNumber: Math.max(modifiedRange.startLineNumber, range.startLineNumber),
      modifiedEndLineNumber: Math.min(modifiedRange.endLineNumber, range.endLineNumber)
    }
  }

  if (rangeContains(range, modifiedRange)) {
    return change
  }

  if (!hasSameLineCount(change)) {
    return null
  }

  const modifiedStartLineNumber = Math.max(modifiedRange.startLineNumber, range.startLineNumber)
  const modifiedEndLineNumber = Math.min(modifiedRange.endLineNumber, range.endLineNumber)

  const delta = modifiedStartLineNumber - change.modifiedStartLineNumber
  const length = modifiedEndLineNumber - modifiedStartLineNumber
  return {
    originalStartLineNumber: change.originalStartLineNumber + delta,
    originalEndLineNumber: change.originalStartLineNumber + delta + length,
    modifiedStartLineNumber,
    modifiedEndLineNumber
  }
}

export function selectedLineChanges(
  changes: readonly LineChange[],
  ranges: readonly LineSelectionRange[],
  modifiedLineCount: number,
  content?: LineChangeContent
): LineChange[] {
  const selected: LineChange[] = []
  for (const change of refineLineChanges(changes, content)) {
    for (const range of ranges) {
      const selectedChange = intersectLineChangeWithRange(change, range, modifiedLineCount)
      if (selectedChange) {
        selected.push(selectedChange)
      }
    }
  }
  return selected.sort(compareLineChanges)
}

export function lineChangesOutsideRanges(
  changes: readonly LineChange[],
  ranges: readonly LineSelectionRange[],
  modifiedLineCount: number,
  content?: LineChangeContent
): LineChange[] {
  const remaining: LineChange[] = []

  for (const change of refineLineChanges(changes, content)) {
    const modifiedRange = modifiedRangeForLineChange(change, modifiedLineCount)
    if (!ranges.some((range) => rangesIntersect(modifiedRange, range))) {
      remaining.push(change)
      continue
    }

    const outsideRanges = subtractRanges(modifiedRange, ranges)
    if (outsideRanges.length === 0) continue

    if (!canSliceByModifiedRange(change)) {
      remaining.push(change)
      continue
    }

    for (const range of outsideRanges) {
      const outsideChange = intersectLineChangeWithRange(change, range, modifiedLineCount)
      if (outsideChange) remaining.push(outsideChange)
    }
  }

  return remaining.sort(compareLineChanges)
}
