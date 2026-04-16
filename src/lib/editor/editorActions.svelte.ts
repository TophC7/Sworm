// Reactive bridge between Monaco editors and the app command palette.
//
// Actions persist as long as an editor tab is mounted — NOT cleared on
// blur, because blur fires when the command palette opens and the user
// needs those actions visible to select one.

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
    .filter((a) => {
      if (HIDDEN_ACTIONS.has(a.id)) return false
      return !HIDDEN_PREFIXES.some((prefix) => a.id.startsWith(prefix))
    })
    .map((a) => ({ id: a.id, label: a.label, run: () => a.run() }))
}

/** Populate actions on editor creation. */
export function registerEditor(editorInstance: editor.IStandaloneCodeEditor) {
  cancelBlurTimer()
  populateActions(editorInstance)
}

/** Mark as focused; re-populates actions if a different editor gained focus. */
export function onEditorFocus(editorInstance: editor.IStandaloneCodeEditor) {
  cancelBlurTimer()
  editorFocused = true
  if (registeredInstance !== editorInstance) {
    populateActions(editorInstance)
  }
}

/** Mark as blurred after a delay (survives command palette steal). */
export function onEditorBlur() {
  cancelBlurTimer()
  blurTimer = setTimeout(() => {
    editorFocused = false
    blurTimer = null
  }, 400)
}

/** Clear on unmount — only if this instance was the registered one. */
export function onEditorDestroy(editorInstance: editor.IStandaloneCodeEditor) {
  cancelBlurTimer()
  if (registeredInstance === editorInstance) {
    editorFocused = false
    actions = []
    registeredInstance = null
  }
}

export function isEditorFocused(): boolean {
  return editorFocused
}

export function hasEditorActions(): boolean {
  return actions.length > 0
}

export function getEditorActions(): EditorAction[] {
  return actions
}
