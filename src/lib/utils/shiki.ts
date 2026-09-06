// Shared Shiki highlighter singleton with CSS-variables theme.
//
// Used by markdown previews; @shikijs/rehype loads languages on demand.
//
// The Monaco editor has its own separate Shiki instance (monacoEnv.ts)
// because it uses a different theme format (ThemeRegistration with
// hardcoded hex colors) and feeds into shikiToMonaco.

import type { Highlighter } from 'shiki'

export const SHIKI_THEME_NAME = 'sworm'

let highlighterPromise: Promise<Highlighter> | null = null
let highlighterInstance: Highlighter | null = null

/** Get or create the singleton Shiki highlighter (no preloaded languages). */
export async function getHighlighter(): Promise<Highlighter> {
  if (highlighterInstance) return highlighterInstance

  if (!highlighterPromise) {
    highlighterPromise = import('shiki').then(
      async ({ createHighlighter, createCssVariablesTheme, createJavaScriptRegexEngine }) => {
        const theme = createCssVariablesTheme({
          name: SHIKI_THEME_NAME,
          variablePrefix: '--shiki-'
        })
        const h = await createHighlighter({
          themes: [theme],
          langs: [],
          // Match the Monaco integration and avoid the Oniguruma wasm
          // engine path that has been crashing in the desktop runtime.
          engine: createJavaScriptRegexEngine({ forgiving: true })
        })
        highlighterInstance = h
        return h
      }
    )
  }

  return highlighterPromise
}
