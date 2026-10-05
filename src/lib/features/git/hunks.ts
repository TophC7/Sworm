import type { LineChange } from '$lib/features/git/lineChanges'

export type ChangeHunkKind = 'add' | 'delete' | 'modify'

export interface ChangeHunk extends LineChange {
  id: string
  kind: ChangeHunkKind
}

type Monaco = typeof import('monaco-editor')
type IStandaloneDiffEditor = import('monaco-editor').editor.IStandaloneDiffEditor
type ILineChange = import('monaco-editor').editor.ILineChange

const DIFF_MAX_COMPUTATION_MS = 5000

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
      maxComputationTime: DIFF_MAX_COMPUTATION_MS,
      hideUnchangedRegions: { enabled: false }
    })
  }
  return computeEditor
}

function kindOf(change: ILineChange): ChangeHunkKind {
  if (change.originalEndLineNumber === 0) return 'add'
  if (change.modifiedEndLineNumber === 0) return 'delete'
  return 'modify'
}

function toHunks(changes: readonly ILineChange[]): ChangeHunk[] {
  return changes.map((change, index) => {
    const kind = kindOf(change)
    return {
      id: `${index}:${change.originalStartLineNumber}:${change.modifiedStartLineNumber}`,
      kind,
      originalStartLineNumber: change.originalStartLineNumber,
      originalEndLineNumber: change.originalEndLineNumber,
      modifiedStartLineNumber: change.modifiedStartLineNumber,
      modifiedEndLineNumber: change.modifiedEndLineNumber
    }
  })
}

async function computeNow(
  monaco: Monaco,
  originalContent: string,
  modifiedContent: string,
  language: string
): Promise<ChangeHunk[] | null> {
  if (originalContent === modifiedContent) return []

  const editor = ensureEditor(monaco)
  const original = monaco.editor.createModel(originalContent, language)
  const modified = monaco.editor.createModel(modifiedContent, language)

  try {
    const waitForDiff = new Promise<boolean>((resolve) => {
      let settled = false
      let off: { dispose(): void } | null = null
      let timer: number | null = null
      const finish = (completed: boolean) => {
        if (settled) return
        settled = true
        off?.dispose()
        if (timer) window.clearTimeout(timer)
        resolve(completed)
      }
      off = editor.onDidUpdateDiff(() => finish(true))
      timer = window.setTimeout(() => finish(false), DIFF_MAX_COMPUTATION_MS + 1000)
    })

    editor.setModel({ original, modified })
    if (!(await waitForDiff)) return null
    return toHunks(editor.getLineChanges() ?? [])
  } finally {
    editor.setModel(null)
    original.dispose()
    modified.dispose()
  }
}

/** Null means superseded or timed out; callers keep their previous hunks. */
export function computeChangeHunks(
  monaco: Monaco,
  originalContent: string,
  modifiedContent: string,
  language = 'plaintext',
  isCurrent: () => boolean = () => true
): Promise<ChangeHunk[] | null> {
  const run = () => (isCurrent() ? computeNow(monaco, originalContent, modifiedContent, language) : null)
  const task = computeChain.then(run, run)
  computeChain = task.catch(() => undefined)
  return task
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
