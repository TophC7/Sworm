// Shared Shiki highlighter singleton with CSS-variables theme.
//
// Used by both the diff viewer (codeToHast) and the markdown preview
// (codeToHtml). Languages are loaded on demand — the first call for
// a new language triggers an async grammar fetch; subsequent calls
// for the same language are deduplicated.
//
// The Monaco editor has its own separate Shiki instance (monacoEnv.ts)
// because it uses a different theme format (ThemeRegistration with
// hardcoded hex colors) and feeds into shikiToMonaco.

import type { Highlighter } from 'shiki'

export const SHIKI_THEME_NAME = 'sworm'

let highlighterPromise: Promise<Highlighter> | null = null
let highlighterInstance: Highlighter | null = null

/** Track in-flight language loads so concurrent requests share one promise. */
const langLoadPromises = new Map<string, Promise<boolean>>()

/** Get or create the singleton Shiki highlighter (no preloaded languages). */
export async function getHighlighter(): Promise<Highlighter> {
  if (highlighterInstance) return highlighterInstance

  if (!highlighterPromise) {
    highlighterPromise = import('shiki').then(async ({ createHighlighter, createCssVariablesTheme }) => {
      const theme = createCssVariablesTheme({
        name: SHIKI_THEME_NAME,
        variablePrefix: '--shiki-'
      })
      const h = await createHighlighter({ themes: [theme], langs: [] })
      highlighterInstance = h
      return h
    })
  }

  return highlighterPromise
}

/**
 * Ensure a language grammar is loaded. Returns true if available after
 * this call. Concurrent calls for the same language share one load.
 */
export async function ensureLanguage(lang: string): Promise<boolean> {
  if (lang === 'plaintext') return false

  const h = await getHighlighter()
  if (h.getLoadedLanguages().includes(lang)) return true

  const existing = langLoadPromises.get(lang)
  if (existing) return existing

  const promise = (async () => {
    try {
      await h.loadLanguage(lang as Parameters<typeof h.loadLanguage>[0])
      return true
    } catch {
      return false
    } finally {
      langLoadPromises.delete(lang)
    }
  })()

  langLoadPromises.set(lang, promise)
  return promise
}
