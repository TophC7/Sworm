export type ChangeHunkKind = 'add' | 'delete' | 'modify'

export interface ChangeHunk {
  id: string
  kind: ChangeHunkKind
  originalStartLineNumber: number
  originalEndLineNumber: number
  modifiedStartLineNumber: number
  modifiedEndLineNumber: number
  originalLines: string[]
  modifiedLines: string[]
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

type Monaco = typeof import('monaco-editor')
type IStandaloneDiffEditor = import('monaco-editor').editor.IStandaloneDiffEditor
type ILineChange = import('monaco-editor').editor.ILineChange

let computeHost: HTMLDivElement | null = null
let computeEditor: IStandaloneDiffEditor | null = null
let computeChain: Promise<unknown> = Promise.resolve()

function ensureHost(): HTMLDivElement {
  if (!computeHost) {
    computeHost = document.createElement('div')
    computeHost.setAttribute('aria-hidden', 'true')
    computeHost.style.cssText = [
      'position:absolute',
      'left:-99999px',
      'top:-99999px',
      'width:800px',
      'height:600px',
      'overflow:hidden',
      'pointer-events:none',
      'visibility:hidden'
    ].join(';')
    document.body.appendChild(computeHost)
  }
  return computeHost
}

function ensureEditor(monaco: Monaco): IStandaloneDiffEditor {
  if (!computeEditor) {
    computeEditor = monaco.editor.createDiffEditor(ensureHost(), {
      renderSideBySide: false,
      readOnly: true,
      minimap: { enabled: false },
      renderOverviewRuler: false,
      automaticLayout: false,
      scrollBeyondLastLine: false,
      renderIndicators: false,
      renderMarginRevertIcon: false,
      contextmenu: false,
      hideUnchangedRegions: { enabled: false }
    })
  }
  return computeEditor
}

function splitLines(content: string): string[] {
  if (content.length === 0) return []
  return content.split(/\r\n|\r|\n/)
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

function lineSlice(lines: string[], start: number, end: number): string[] {
  if (start <= 0 || end <= 0 || end < start) return []
  return lines.slice(start - 1, end)
}

function kindOf(change: ILineChange): ChangeHunkKind {
  if (change.originalEndLineNumber === 0) return 'add'
  if (change.modifiedEndLineNumber === 0) return 'delete'
  return 'modify'
}

function toHunks(originalContent: string, modifiedContent: string, changes: readonly ILineChange[]): ChangeHunk[] {
  const originalLines = splitLines(originalContent)
  const modifiedLines = splitLines(modifiedContent)

  return changes.map((change, index) => {
    const kind = kindOf(change)
    return {
      id: `${index}:${change.originalStartLineNumber}:${change.modifiedStartLineNumber}`,
      kind,
      originalStartLineNumber: change.originalStartLineNumber,
      originalEndLineNumber: change.originalEndLineNumber,
      modifiedStartLineNumber: change.modifiedStartLineNumber,
      modifiedEndLineNumber: change.modifiedEndLineNumber,
      originalLines: lineSlice(originalLines, change.originalStartLineNumber, change.originalEndLineNumber),
      modifiedLines: lineSlice(modifiedLines, change.modifiedStartLineNumber, change.modifiedEndLineNumber)
    }
  })
}

async function computeNow(
  monaco: Monaco,
  originalContent: string,
  modifiedContent: string,
  language: string
): Promise<ChangeHunk[]> {
  if (originalContent === modifiedContent) return []

  const editor = ensureEditor(monaco)
  const original = monaco.editor.createModel(originalContent, language)
  const modified = monaco.editor.createModel(modifiedContent, language)

  try {
    const waitForDiff = new Promise<void>((resolve) => {
      let settled = false
      let off: { dispose(): void } | null = null
      let timer: number | null = null
      const finish = () => {
        if (settled) return
        settled = true
        off?.dispose()
        if (timer) window.clearTimeout(timer)
        resolve()
      }
      off = editor.onDidUpdateDiff(finish)
      timer = window.setTimeout(finish, 1000)
    })

    editor.setModel({ original, modified })
    await waitForDiff
    return toHunks(originalContent, modifiedContent, editor.getLineChanges() ?? [])
  } finally {
    editor.setModel(null)
    original.dispose()
    modified.dispose()
  }
}

export function computeChangeHunks(
  monaco: Monaco,
  originalContent: string,
  modifiedContent: string,
  language = 'plaintext'
): Promise<ChangeHunk[]> {
  const task = computeChain.then(
    () => computeNow(monaco, originalContent, modifiedContent, language),
    () => computeNow(monaco, originalContent, modifiedContent, language)
  )
  computeChain = task.catch(() => undefined)
  return task
}

export function hunkLabel(hunk: ChangeHunk): string {
  switch (hunk.kind) {
    case 'add':
      return `Added ${lineRangeLabel(hunk.modifiedStartLineNumber, hunk.modifiedEndLineNumber)}`
    case 'delete':
      return `Deleted after line ${Math.max(1, hunk.modifiedStartLineNumber)}`
    case 'modify':
      return `Changed ${lineRangeLabel(hunk.modifiedStartLineNumber, hunk.modifiedEndLineNumber)}`
  }
}

export function lineRangeLabel(start: number, end: number): string {
  if (start <= 0 && end <= 0) return 'line 1'
  if (end <= 0 || end === start) return `line ${Math.max(1, start)}`
  return `lines ${start}-${end}`
}

export function hunkModifiedText(hunk: ChangeHunk): string {
  return hunk.modifiedLines.join('\n')
}

export function hunkOriginalText(hunk: ChangeHunk): string {
  return hunk.originalLines.join('\n')
}

export function hunkRevealLine(hunk: ChangeHunk): number {
  return Math.max(1, hunk.modifiedStartLineNumber || hunk.originalStartLineNumber || 1)
}

export function lineIntersectsHunk(lineNumber: number, hunk: ChangeHunk): boolean {
  if (lineNumber === 1 && hunk.modifiedStartLineNumber === 0 && hunk.modifiedEndLineNumber === 0) {
    return true
  }
  const start = Math.max(1, hunk.modifiedStartLineNumber)
  const end = Math.max(start, hunk.modifiedEndLineNumber || hunk.modifiedStartLineNumber)
  return lineNumber >= start && lineNumber <= end
}

export function hunksIntersectOrTouch(a: ChangeHunk, b: ChangeHunk): boolean {
  const aStart = Math.max(1, a.modifiedStartLineNumber)
  const aEnd = Math.max(aStart, a.modifiedEndLineNumber || a.modifiedStartLineNumber)
  const bStart = Math.max(1, b.modifiedStartLineNumber)
  const bEnd = Math.max(bStart, b.modifiedEndLineNumber || b.modifiedStartLineNumber)
  return aStart <= bEnd + 1 && bStart <= aEnd + 1
}

export function compareHunks(a: ChangeHunk, b: ChangeHunk): number {
  return (
    a.modifiedStartLineNumber - b.modifiedStartLineNumber ||
    a.modifiedEndLineNumber - b.modifiedEndLineNumber ||
    a.originalStartLineNumber - b.originalStartLineNumber ||
    a.originalEndLineNumber - b.originalEndLineNumber
  )
}

export function applyChangeHunks(
  originalContent: string,
  modifiedContent: string,
  hunksToApply: readonly ChangeHunk[]
): string {
  if (hunksToApply.length === 0) return originalContent

  const original = new TextSnapshot(originalContent)
  const modified = new TextSnapshot(modifiedContent)
  const result: string[] = []
  let currentLine = 0

  for (const hunk of [...hunksToApply].sort(compareHunks)) {
    const isInsertion = hunk.originalEndLineNumber === 0
    const isDeletion = hunk.modifiedEndLineNumber === 0

    let endLine = isInsertion ? hunk.originalStartLineNumber : hunk.originalStartLineNumber - 1
    let endCharacter = 0

    if (isDeletion && hunk.originalEndLineNumber === original.lineCount) {
      endLine -= 1
      endCharacter = endLine < 0 ? 0 : original.lineLength(endLine)
      endLine = Math.max(0, endLine)
    }

    result.push(original.textInRange(currentLine, 0, endLine, endCharacter))

    if (!isDeletion) {
      let fromLine = hunk.modifiedStartLineNumber - 1
      let fromCharacter = 0

      if (isInsertion && hunk.originalStartLineNumber === original.lineCount) {
        fromLine -= 1
        fromCharacter = fromLine < 0 ? 0 : modified.lineLength(fromLine)
        fromLine = Math.max(0, fromLine)
      }

      result.push(modified.textInRange(fromLine, fromCharacter, hunk.modifiedEndLineNumber, 0))
    }

    currentLine = isInsertion ? hunk.originalStartLineNumber : hunk.originalEndLineNumber
  }

  result.push(original.textInRange(currentLine, 0, original.lineCount, 0))
  return result.join('')
}
