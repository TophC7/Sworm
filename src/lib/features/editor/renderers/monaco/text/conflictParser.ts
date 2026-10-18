const START_MARKER = '<<<<<<<'
const COMMON_MARKER = '|||||||'
const SPLIT_MARKER = '======='
const END_MARKER = '>>>>>>>'

export interface ConflictBlock {
  startLine: number
  startHeader: string
  commonLine: number | null
  commonHeader: string | null
  splitterLine: number
  endLine: number
  endHeader: string
}

export type ConflictResolution = 'current' | 'incoming' | 'both'

function markerHeader(line: string, marker: string): string | null {
  if (line === marker) return ''
  if (line.startsWith(`${marker} `) || line.startsWith(`${marker}\t`)) {
    return line.slice(marker.length).trim()
  }
  return null
}

export function parseConflictBlocks(lines: string[]): ConflictBlock[] {
  const blocks: ConflictBlock[] = []
  let pending: ConflictBlock | null = null

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index].replace(/\r$/, '')
    const lineNumber = index + 1
    const startHeader = markerHeader(line, START_MARKER)
    if (startHeader !== null) {
      // A fresh start recovers from an incomplete or malformed preceding block.
      pending = {
        startLine: lineNumber,
        startHeader,
        commonLine: null,
        commonHeader: null,
        splitterLine: 0,
        endLine: 0,
        endHeader: ''
      }
      continue
    }
    if (!pending) continue

    const commonHeader = markerHeader(line, COMMON_MARKER)
    if (commonHeader !== null) {
      if (pending.commonLine !== null || pending.splitterLine !== 0) pending = null
      else {
        pending.commonLine = lineNumber
        pending.commonHeader = commonHeader
      }
      continue
    }
    if (line === SPLIT_MARKER) {
      if (pending.splitterLine !== 0) pending = null
      else pending.splitterLine = lineNumber
      continue
    }
    const endHeader = markerHeader(line, END_MARKER)
    if (endHeader !== null) {
      if (pending.splitterLine !== 0) {
        blocks.push({ ...pending, endLine: lineNumber, endHeader })
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
