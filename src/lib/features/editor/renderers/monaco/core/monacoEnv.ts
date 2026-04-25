// Monaco environment bootstrap. Imported once as a side effect before
// any editor mounts. Sets up workers, Shiki tokenization for languages
// Monaco lacks (Nix, Svelte, Fish), theme, and keybinding overrides.

import EditorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import CssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import HtmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import JsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import TsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'
import { ensureMonacoFormatters } from '$lib/features/editor/renderers/monaco/core/formatters'
import { attachMonacoKeybindingOverrides } from '$lib/features/editor/renderers/monaco/core/keybindings'
import { ensureMonacoLsp } from '$lib/features/editor/lsp/registry'
import { registerSwormTheme, SWORM_SHIKI_THEME } from '$lib/features/editor/renderers/monaco/core/monacoTheme'
import { attachMonaco } from '$lib/features/editor/schemas/registry'

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

interface TsLanguageDefaults {
  modeConfiguration: {
    documentRangeFormattingEdits?: boolean
    onTypeFormattingEdits?: boolean
  }
  setDiagnosticsOptions(options: {
    noSemanticValidation: boolean
    noSyntaxValidation: boolean
    noSuggestionDiagnostics: boolean
  }): void
  setModeConfiguration(options: { documentRangeFormattingEdits?: boolean; onTypeFormattingEdits?: boolean }): void
}

interface CssLanguageDefaults {
  setDiagnosticsOptions(options: { validate?: boolean }): void
}

interface JsonLanguageDefaults {
  modeConfiguration: {
    documentFormattingEdits?: boolean
    documentRangeFormattingEdits?: boolean
  }
  setModeConfiguration(options: { documentFormattingEdits?: boolean; documentRangeFormattingEdits?: boolean }): void
}

const DISABLED_TS_DIAGNOSTICS = {
  noSemanticValidation: true,
  noSyntaxValidation: true,
  noSuggestionDiagnostics: true
} as const

function getTsLanguageService(monaco: typeof import('monaco-editor')): {
  javascriptDefaults: TsLanguageDefaults
  typescriptDefaults: TsLanguageDefaults
} {
  return monaco.languages.typescript as unknown as {
    javascriptDefaults: TsLanguageDefaults
    typescriptDefaults: TsLanguageDefaults
  }
}

function getCssLanguageServices(monaco: typeof import('monaco-editor')): CssLanguageDefaults[] {
  const cssLanguages = monaco.languages.css as unknown as {
    cssDefaults: CssLanguageDefaults
    scssDefaults: CssLanguageDefaults
    lessDefaults: CssLanguageDefaults
  }

  return [cssLanguages.cssDefaults, cssLanguages.scssDefaults, cssLanguages.lessDefaults]
}

function getJsonLanguageService(monaco: typeof import('monaco-editor')): JsonLanguageDefaults {
  return (monaco.languages.json as unknown as { jsonDefaults: JsonLanguageDefaults }).jsonDefaults
}

/** One-time Monaco setup: Shiki tokenization, theme, keybinding overrides. */
let initPromise: Promise<void> | null = null

export function initMonaco(monaco: typeof import('monaco-editor')): Promise<void> {
  if (initPromise) return initPromise

  initPromise = (async () => {
    const [{ createHighlighter, createJavaScriptRegexEngine }, { shikiToMonaco }] = await Promise.all([
      import('shiki'),
      import('@shikijs/monaco')
    ])

    const highlighter = await createHighlighter({
      themes: [SWORM_SHIKI_THEME],
      langs: [...SHIKI_LANGUAGES],
      // Prefer the pure-JS engine here. The default Oniguruma wasm
      // path has been crashing inside Tauri/WebView during Monaco
      // background tokenization with out-of-bounds memory errors.
      engine: createJavaScriptRegexEngine({ forgiving: true })
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

    // Keep Monaco's built-in language features as fallback when an
    // external LSP is missing or disabled, but suppress TS diagnostics
    // because Monaco's standalone TS worker misreads Sworm's project
    // alias/runtime environment and produces false positives.
    const tsLanguageService = getTsLanguageService(monaco)
    tsLanguageService.javascriptDefaults.setDiagnosticsOptions(DISABLED_TS_DIAGNOSTICS)
    tsLanguageService.typescriptDefaults.setDiagnosticsOptions(DISABLED_TS_DIAGNOSTICS)
    tsLanguageService.javascriptDefaults.setModeConfiguration({
      ...tsLanguageService.javascriptDefaults.modeConfiguration,
      documentRangeFormattingEdits: false,
      onTypeFormattingEdits: false
    })
    tsLanguageService.typescriptDefaults.setModeConfiguration({
      ...tsLanguageService.typescriptDefaults.modeConfiguration,
      documentRangeFormattingEdits: false,
      onTypeFormattingEdits: false
    })

    const jsonLanguageService = getJsonLanguageService(monaco)
    jsonLanguageService.setModeConfiguration({
      ...jsonLanguageService.modeConfiguration,
      documentFormattingEdits: false,
      documentRangeFormattingEdits: false
    })

    // CSS diagnostics stay off even without an external LSP so Tailwind
    // at-rules like @source and @apply do not regress to false errors.
    for (const cssLanguageService of getCssLanguageServices(monaco)) {
      cssLanguageService.setDiagnosticsOptions({ validate: false })
    }

    // Language-specific configuration (after Shiki registers language IDs)
    const { registerNixLanguage } = await import('$lib/features/editor/renderers/monaco/core/languages/nix')
    registerNixLanguage(monaco)
    await ensureMonacoLsp(monaco)
    await ensureMonacoFormatters(monaco)

    const { KeyMod, KeyCode } = monaco
    monaco.editor.addKeybindingRules([
      { keybinding: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyP, command: '-editor.action.quickCommand' },
      { keybinding: KeyCode.F1, command: '-editor.action.quickCommand' },
      { keybinding: KeyMod.CtrlCmd | KeyMod.Shift | KeyCode.KeyO, command: '-editor.action.quickOutline' }
    ])
    attachMonacoKeybindingOverrides(monaco)

    // Flush any JSON schemas that were registered before Monaco
    // finished loading (bootstrap runs before the first editor mount).
    attachMonaco(monaco)
  })().catch((err) => {
    // Allow retry on next editor mount if Shiki fails to load
    initPromise = null
    throw err
  })

  return initPromise
}
