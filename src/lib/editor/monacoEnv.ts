// Monaco environment bootstrap — imported once as a side-effect before
// any editor mounts. Sets up workers, Shiki tokenization for languages
// Monaco lacks (Nix, Svelte, Fish), theme, and keybinding overrides.

import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import TsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import { registerSwormTheme, SWORM_SHIKI_THEME } from '$lib/editor/monacoTheme'

self.MonacoEnvironment = {
  getWorker(_workerId: string, label: string): Worker {
    if (label === 'typescript' || label === 'javascript') return new TsWorker()
    if (label === 'json') return new JsonWorker()
    if (label === 'css' || label === 'less' || label === 'scss') return new CssWorker()
    if (label === 'html' || label === 'handlebars' || label === 'razor') return new HtmlWorker()
    return new EditorWorker()
  }
}

// Languages where Shiki's TextMate grammars replace Monaco's missing
// or inadequate Monarch tokenizers.
const SHIKI_LANGUAGES = ['nix', 'svelte', 'fish'] as const

/** One-time Monaco setup: Shiki tokenization, theme, keybinding overrides. */
let initPromise: Promise<void> | null = null

export function initMonaco(monaco: typeof import('monaco-editor')): Promise<void> {
  if (initPromise) return initPromise

  initPromise = (async () => {
    const [{ createHighlighter }, { shikiToMonaco }] = await Promise.all([import('shiki'), import('@shikijs/monaco')])

    const highlighter = await createHighlighter({
      themes: [SWORM_SHIKI_THEME],
      langs: [...SHIKI_LANGUAGES]
    })

    for (const lang of SHIKI_LANGUAGES) {
      monaco.languages.register({ id: lang })
    }

    // shikiToMonaco registers a TokensProvider per loaded language and
    // defines its own version of the theme. We call registerSwormTheme
    // after to re-define the theme with merged Monarch + TextMate rules.
    // The Shiki TokensProvider's internal color map stays from the first
    // registration, which is fine because both use the same palette.
    shikiToMonaco(highlighter, monaco)
    registerSwormTheme(monaco)

    // Language-specific configuration (after Shiki registers language IDs)
    const { registerNixLanguage } = await import('$lib/editor/languages/nix')
    registerNixLanguage(monaco)

    const { KeyMod, KeyCode } = monaco
    monaco.editor.addKeybindingRules([
      { keybinding: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyP, command: '-editor.action.quickCommand' },
      { keybinding: KeyCode.F1, command: '-editor.action.quickCommand' },
      { keybinding: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyO, command: '-editor.action.quickOutline' }
    ])
  })().catch((err) => {
    // Allow retry on next editor mount if Shiki fails to load
    initPromise = null
    throw err
  })

  return initPromise
}
