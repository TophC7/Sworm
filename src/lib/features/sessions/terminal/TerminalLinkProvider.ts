import { openLink } from '$lib/features/workbench/links/openLink'
import { mapBufferStringIndex } from './mapBufferStringIndex'
import type { IBufferLine, ILink, ILinkProvider, Terminal } from '@xterm/xterm'

// Web URLs
const URL_REGEX = /https?:\/\/[^\s"'!*(){}|\\^<>`]+[^\s"':,.!?{}|\\^~\[\]`()<>]/gi
// Harness URI schemes: skill://, rule://, local://, issue://, pr://, file://, etc.
// Exclude trailing interpunction (. , ; : ] ) )
const URI_REGEX =
  /\b(?:file|skill|rule|local|issue|pr|artifact|history|omp|agent):\/\/(?:[^\s"'`<>)]*[^^\s"'`<>).,;:\]\\])?/gi
// File paths with optional line/col or range, anchored on a token boundary:
// - src/foo.ts:42:15-80
// - src/foo.ts:42:10
// - src/foo.ts:42
// - src/foo.ts:42-100
// - src/foo.ts#L42
// - [src/foo.ts#1A2B]
// - @scope/pkg/foo.ts
// - ~/dev/app/src/a.ts
const FILE_PATH_REGEX =
  /(?:\[(?:\.\/|\/|[~@a-zA-Z0-9_.-]+\/)*[a-zA-Z0-9_./\-]+\.[a-zA-Z0-9_]+#[0-9a-fA-F]+\]|(?<=[^\w/.-]|^)(?:(?:\.\/|\/|~(?:\/|$)|@[\w-]+\/|[a-zA-Z0-9_.-]+\/)[a-zA-Z0-9_./\-]+\.[a-zA-Z0-9_]+(?:#L?\d+(?:-L?\d+)?|:\d+(?::\d+)?(?:-\d+)?)?))/g

export class TerminalLinkProvider implements ILinkProvider {
  constructor(
    private readonly terminal: Terminal,
    private readonly getFolderPath: () => string | null,
    private readonly getHostEl: () => HTMLElement | null
  ) {}

  provideLinks(y: number, callback: (links: ILink[] | undefined) => void): void {
    const [lines, startLineIndex] = this.getWindowedLineStrings(y - 1)
    const lineText = lines.length === 1 ? lines[0] : lines.join('')
    if (!lineText) {
      callback(undefined)
      return
    }

    const intervals: Array<{ start: number; end: number }> = []
    const links: ILink[] = []

    const hasProtocol = lineText.includes('://')
    if (hasProtocol) {
      this.scanRegex(URL_REGEX, lineText, startLineIndex, intervals, links)
      this.scanRegex(URI_REGEX, lineText, startLineIndex, intervals, links)
    }
    const hasPathMarker =
      lineText.includes('.') &&
      (lineText.includes('/') ||
        lineText.includes('\\') ||
        lineText.includes('[') ||
        lineText.includes('~') ||
        lineText.includes('@'))
    if (hasPathMarker) {
      this.scanRegex(FILE_PATH_REGEX, lineText, startLineIndex, intervals, links)
    }

    callback(links.length > 0 ? links : undefined)
  }

  private scanRegex(
    regex: RegExp,
    text: string,
    startLineIndex: number,
    intervals: Array<{ start: number; end: number }>,
    links: ILink[]
  ): void {
    regex.lastIndex = 0
    let match: RegExpExecArray | null

    while ((match = regex.exec(text))) {
      const matchText = match[0]
      const startIndex = match.index
      const endIndex = startIndex + matchText.length

      // Check for overlap
      let overlaps = false
      for (const inv of intervals) {
        if (Math.max(startIndex, inv.start) < Math.min(endIndex, inv.end)) {
          overlaps = true
          break
        }
      }
      if (overlaps) continue

      const [startY, startX] = mapBufferStringIndex(this.terminal, startLineIndex, 0, startIndex)
      const [endY, endX] = mapBufferStringIndex(this.terminal, startY, startX, matchText.length)

      if (startY === -1 || startX === -1 || endY === -1 || endX === -1) {
        continue
      }

      intervals.push({ start: startIndex, end: endIndex })

      const range = {
        start: { x: startX + 1, y: startY + 1 },
        end: { x: endX, y: endY + 1 }
      }

      links.push({
        range,
        text: matchText,
        activate: (event: MouseEvent, targetText: string) => {
          if (event.ctrlKey || event.metaKey) {
            const folderPath = this.getFolderPath()
            void openLink(targetText, folderPath)
          }
        },
        hover: (_event: MouseEvent, targetText: string) => {
          const hostEl = this.getHostEl()
          if (hostEl) {
            hostEl.title = `Ctrl+click to follow: ${targetText}`
          }
        },
        leave: () => {
          const hostEl = this.getHostEl()
          if (hostEl) {
            hostEl.title = ''
          }
        }
      })
    }
  }

  private getWindowedLineStrings(lineIndex: number): [string[], number] {
    let line: IBufferLine | undefined
    let topIdx = lineIndex
    let bottomIdx = lineIndex
    const lines: string[] = []

    if ((line = this.terminal.buffer.active.getLine(lineIndex))) {
      const currentContent = line.translateToString(true)

      // Expand top when wrapped
      if (line.isWrapped && currentContent[0] !== ' ') {
        let len = 0
        while ((line = this.terminal.buffer.active.getLine(--topIdx)) && len < 2048) {
          const content = line.translateToString(true)
          len += content.length
          lines.push(content)
          if (!line.isWrapped || content.indexOf(' ') !== -1) break
        }
        lines.reverse()
      }

      lines.push(currentContent)

      // Expand bottom when wrapped
      let len = 0
      while ((line = this.terminal.buffer.active.getLine(++bottomIdx)) && line.isWrapped && len < 2048) {
        const content = line.translateToString(true)
        len += content.length
        lines.push(content)
        if (content.indexOf(' ') !== -1) break
      }
    }

    return [lines, topIdx]
  }
}
