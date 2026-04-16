// Shiki code block highlighting for the markdown preview.
//
// Uses the shared Shiki singleton ($lib/shiki.ts) which is also
// used by the diff viewer. Only the output differs: codeToHtml
// here vs codeToHast in the diff adapter.

import { getHighlighter, ensureLanguage, SHIKI_THEME_NAME } from '$lib/utils/shiki'

/**
 * Highlight a code string, returning raw HTML.
 * Loads the language grammar on demand. Returns null if the language
 * isn't supported or is plaintext (caller renders plain fallback).
 */
export async function highlightCode(code: string, lang?: string): Promise<string | null> {
  if (!lang || lang === 'plaintext' || lang === 'text') return null

  const loaded = await ensureLanguage(lang)
  if (!loaded) return null

  const h = await getHighlighter()
  return h.codeToHtml(code, { lang, theme: SHIKI_THEME_NAME })
}
