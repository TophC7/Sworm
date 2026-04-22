// Reactive bridge between Monaco text editors and the app command palette.
//
// Actions persist as long as a text surface is mounted. Blur is delayed
// so the command palette can steal focus without losing the active
// editor command set.

import type { editor } from 'monaco-editor'

export interface EditorAction {
  id: string
  label: string
  run: () => void
}

const HIDDEN_ACTIONS = new Set([
  'editor.action.quickCommand',
  'editor.action.quickOutline',
  'editor.action.selectAll',
  'editor.action.clipboardCopyAction',
  'editor.action.clipboardCutAction',
  'editor.action.clipboardPasteAction',
  'undo',
  'redo',
  'cursorUndo',
  'cursorRedo',
  'editor.action.setSelectionAnchor',
  'editor.action.webvieweditor.showFind'
])

const HIDDEN_PREFIXES = ['_', 'deleteWord', 'cursor', 'lineBreak', 'tab', 'outdent', 'type']

let actions = $state<EditorAction[]>([])
let editorFocused = $state(false)
let blurTimer: ReturnType<typeof setTimeout> | null = null
let registeredInstance: editor.IStandaloneCodeEditor | null = null

function cancelBlurTimer() {
  if (blurTimer) {
    clearTimeout(blurTimer)
    blurTimer = null
  }
}

function populateActions(editorInstance: editor.IStandaloneCodeEditor) {
  registeredInstance = editorInstance
  actions = editorInstance
    .getSupportedActions()
    .filter((action) => {
      if (HIDDEN_ACTIONS.has(action.id)) return false
      return !HIDDEN_PREFIXES.some((prefix) => action.id.startsWith(prefix))
    })
    .map((action) => ({ id: action.id, label: action.label, run: () => action.run() }))
}

export function registerTextEditorActions(editorInstance: editor.IStandaloneCodeEditor) {
  cancelBlurTimer()
  populateActions(editorInstance)
}

export function onTextEditorFocus(editorInstance: editor.IStandaloneCodeEditor) {
  cancelBlurTimer()
  editorFocused = true
  if (registeredInstance !== editorInstance) {
    populateActions(editorInstance)
  }
}

export function onTextEditorBlur() {
  cancelBlurTimer()
  blurTimer = setTimeout(() => {
    editorFocused = false
    blurTimer = null
  }, 400)
}

export function onTextEditorDestroy(editorInstance: editor.IStandaloneCodeEditor) {
  cancelBlurTimer()
  if (registeredInstance === editorInstance) {
    editorFocused = false
    actions = []
    registeredInstance = null
  }
}

export function isTextEditorFocused(): boolean {
  return editorFocused
}

export function getTextEditorActions(): EditorAction[] {
  return actions
}
