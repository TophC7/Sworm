import type { Terminal } from '@xterm/xterm'

/**
 * Map a string index back to buffer positions [lineIndex, columnIndex] 0-based.
 * Returns [-1, -1] if line does not exist.
 * Upstream xterm LinkComputer logic: decrements remaining by character width
 * and returns the cell where remaining becomes strictly negative (< 0).
 */
export function mapBufferStringIndex(
  terminal: Terminal,
  lineIndex: number,
  rowIndex: number,
  stringIndex: number
): [number, number] {
  const buf = terminal.buffer.active
  const cell = buf.getNullCell()
  let start = rowIndex
  let remaining = stringIndex

  while (remaining >= 0) {
    const line = buf.getLine(lineIndex)
    if (!line) return [-1, -1]

    for (let i = start; i < line.length; ++i) {
      line.getCell(i, cell)
      const chars = cell.getChars()
      const width = cell.getWidth()
      if (width) {
        remaining -= chars.length || 1

        if (i === line.length - 1 && chars === '') {
          const nextLine = buf.getLine(lineIndex + 1)
          if (nextLine && nextLine.isWrapped) {
            nextLine.getCell(0, cell)
            if (cell.getWidth() === 2) remaining += 1
          }
        }
      }
      if (remaining < 0) {
        return [lineIndex, i]
      }
    }
    lineIndex++
    start = 0
  }
  return [lineIndex, start]
}
