// Monaco environment bootstrap — imported once as a side-effect before
// any editor mounts. Sets up workers, theme, and keybinding overrides.

import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import TsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import { registerSwormTheme } from '$lib/editor/monacoTheme'

self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string): Worker {
    if (label === 'typescript' || label === 'javascript') return new TsWorker()
    if (label === 'json') return new JsonWorker()
    if (label === 'css' || label === 'less' || label === 'scss') return new CssWorker()
    if (label === 'html' || label === 'handlebars' || label === 'razor') return new HtmlWorker()
    return new EditorWorker()
  }
}

/** One-time Monaco setup: theme + keybinding overrides. */
let initialized = false

export function initMonaco(monaco: typeof import('monaco-editor')) {
  if (initialized) return
  initialized = true

  registerSwormTheme(monaco)

  const { KeyMod, KeyCode } = monaco
  monaco.editor.addKeybindingRules([
    { keybinding: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyP, command: '-editor.action.quickCommand' },
    { keybinding: KeyCode.F1, command: '-editor.action.quickCommand' },
    { keybinding: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyO, command: '-editor.action.quickOutline' }
  ])
}
