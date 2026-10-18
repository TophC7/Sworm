const START_MARKER = '<<<<<<<'
const COMMON_MARKER = '|||||||'
const SPLIT_MARKER = '======='
const END_MARKER = '>>>>>>>'

export interface ConflictBlock {
  startLine: number
  commonLine: number | null
  splitterLine: number
  endLine: number
}

export type ConflictResolution = 'current' | 'incoming' | 'both'

function isMarker(line: string, marker: string): boolean {
  return line === marker || line.startsWith(`${marker} `) || line.startsWith(`${marker}\t`)
}

export function parseConflictBlocks(lines: string[]): ConflictBlock[] {
  const blocks: ConflictBlock[] = []
  let pending: ConflictBlock | null = null

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index].replace(/\r$/, '')
    const lineNumber = index + 1
    if (isMarker(line, START_MARKER)) {
      // A fresh start recovers from an incomplete or malformed preceding block.
      pending = { startLine: lineNumber, commonLine: null, splitterLine: 0, endLine: 0 }
      continue
    }
    if (!pending) continue

    if (isMarker(line, COMMON_MARKER)) {
      if (pending.commonLine !== null || pending.splitterLine !== 0) pending = null
      else pending.commonLine = lineNumber
      continue
    }
    if (line === SPLIT_MARKER) {
      if (pending.splitterLine !== 0) pending = null
      else pending.splitterLine = lineNumber
      continue
    }
    if (isMarker(line, END_MARKER)) {
      if (pending.splitterLine !== 0) {
        blocks.push({ ...pending, endLine: lineNumber })
      }
      pending = null
    }
  }
  return blocks
}

/** Keep the chosen content's line terminators; remove all marker/ancestor lines. */
export function resolveConflictBlock(
  lines: string[],
  block: ConflictBlock,
  resolution: ConflictResolution,
  eol = lines.some((line) => line.endsWith('\r')) ? '\r\n' : '\n'
): string {
  const current = lines.slice(block.startLine, (block.commonLine ?? block.splitterLine) - 1)
  const incoming = lines.slice(block.splitterLine, block.endLine - 1)
  const selected = resolution === 'current' ? current : resolution === 'incoming' ? incoming : [...current, ...incoming]
  const text = selected.map((line) => line.replace(/\r$/, '')).join(eol)
  return selected.length > 0 ? text + eol : ''
}

export function getConflictEditRange(block: ConflictBlock, totalLines: number) {
  return {
    startLineNumber: block.startLine,
    startColumn: 1,
    endLineNumber: block.endLine < totalLines ? block.endLine + 1 : block.endLine,
    // Monaco's validateRange clamps this sentinel to the actual EOF column.
    endColumn: block.endLine < totalLines ? 1 : Number.MAX_SAFE_INTEGER
  }
}
