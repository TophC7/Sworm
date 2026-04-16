// Indent Rainbow — colorizes leading whitespace with per-level background tints.
//
// Adapted from vscode-indent-rainbow. Uses Monaco's decorations API to
// paint subtle backgrounds on each indentation level, cycling through 4
// colors. Flags indentation errors and mixed tabs/spaces.

import type { editor as Editor, IDisposable } from 'monaco-editor'

// ── Colors ──────────────────────────────────────────────────────────
// Derived from the Sworm syntax palette at low opacity so the rainbow
// feels native against the #131313 editor background.

const COLORS = [
  'rgba(127,186,255,0.07)', // blue   (#7fbaff)
  'rgba(152,255,127,0.07)', // green  (#98ff7f)
  'rgba(214,172,255,0.07)', // purple (#d6acff)
  'rgba(158,255,239,0.07)' // cyan   (#9effef)
]
const INVALID_COLOR = 'rgba(255,118,114,0.25)' // red (#ff7672) — used for both errors and tab/space mixing

const EXCLUDED_LANGUAGES = new Set(['plaintext'])

// ── Reactive enabled state ──────────────────────────────────────────

let enabled = $state(true)

export function isIndentRainbowEnabled(): boolean {
  return enabled
}

export function toggleIndentRainbow() {
  enabled = !enabled
}

// ── CSS injection ───────────────────────────────────────────────────

function injectStyles() {
  if (document.querySelector('[data-indent-rainbow]')) return

  const style = document.createElement('style')
  style.setAttribute('data-indent-rainbow', '')
  // className decorations render as absolutely-positioned divs in
  // Monaco's .view-overlays layer — they fill the full line height
  // natively, no CSS hacks needed.
  style.textContent =
    COLORS.map((c, i) => `.indent-rainbow-${i} { background-color: ${c}; }`).join('\n') +
    `
    .indent-rainbow-error { background-color: ${INVALID_COLOR}; }
    .indent-rainbow-tabmix { background-color: ${INVALID_COLOR}; }
  `
  document.head.appendChild(style)
}

// ── Core ────────────────────────────────────────────────────────────

export interface IndentRainbowHandle extends IDisposable {
  scheduleUpdate: () => void
}

export function attachIndentRainbow(editor: Editor.IStandaloneCodeEditor): IndentRainbowHandle {
  injectStyles()

  const collection = editor.createDecorationsCollection()
  let timer: ReturnType<typeof setTimeout> | null = null

  function update() {
    const model = editor.getModel()
    if (!model || !enabled) {
      collection.clear()
      return
    }

    const lang = model.getLanguageId()
    if (EXCLUDED_LANGUAGES.has(lang)) {
      collection.clear()
      return
    }

    const tabSize = model.getOptions().tabSize
    const lineCount = model.getLineCount()
    const indentRe = /^[\t ]+/

    const decorations: Editor.IModelDeltaDecoration[] = []

    for (let lineNumber = 1; lineNumber <= lineCount; lineNumber++) {
      const lineText = model.getLineContent(lineNumber)
      const match = indentRe.exec(lineText)
      if (!match) continue

      const raw = match[0]

      // Count effective width to detect misaligned indentation
      let totalSpaces = 0
      for (const ch of raw) totalSpaces += ch === '\t' ? tabSize : 1
      const isError = totalSpaces % tabSize !== 0

      if (isError) {
        decorations.push({
          range: { startLineNumber: lineNumber, startColumn: 1, endLineNumber: lineNumber, endColumn: raw.length + 1 },
          options: { className: 'indent-rainbow-error' }
        })
        continue
      }

      // Walk character by character, one indent level per tabSize spaces or 1 tab
      let col = 1
      let level = 0
      let i = 0

      while (i < raw.length) {
        const ch = raw[i]
        const stepChars = Math.min(ch === '\t' ? 1 : tabSize, raw.length - i)
        const endCol = col + stepChars

        // Detect mixed tabs/spaces within this level
        const slice = raw.slice(i, i + stepChars)
        const hasTabs = slice.includes('\t')
        const hasSpaces = slice.includes(' ')

        if (hasTabs && hasSpaces) {
          decorations.push({
            range: { startLineNumber: lineNumber, startColumn: col, endLineNumber: lineNumber, endColumn: endCol },
            options: { className: 'indent-rainbow-tabmix' }
          })
        } else {
          decorations.push({
            range: { startLineNumber: lineNumber, startColumn: col, endLineNumber: lineNumber, endColumn: endCol },
            options: { className: `indent-rainbow-${level % COLORS.length}` }
          })
        }

        col = endCol
        i += stepChars
        level++
      }
    }

    collection.set(decorations)
  }

  function scheduleUpdate() {
    if (timer) clearTimeout(timer)
    timer = setTimeout(update, 100)
  }

  const contentDisposable = editor.onDidChangeModelContent(scheduleUpdate)
  const langDisposable = editor.onDidChangeModelLanguage(scheduleUpdate)

  // Initial paint
  update()

  return {
    scheduleUpdate,
    dispose() {
      contentDisposable.dispose()
      langDisposable.dispose()
      if (timer) clearTimeout(timer)
      collection.clear()
    }
  }
}
