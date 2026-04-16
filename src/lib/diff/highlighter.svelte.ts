/**
 * Diff viewer syntax highlighting adapter.
 *
 * Wraps the shared Shiki singleton ($lib/shiki.ts) as a
 * DiffFileHighlighter compatible with @git-diff-view/core's
 * `initSyntax()`. Languages are loaded on demand — the first
 * render shows plain text + char-level diff highlights, then
 * syntax coloring appears asynchronously.
 */

import { getHighlighter, ensureLanguage, SHIKI_THEME_NAME } from '$lib/utils/shiki'
import { processAST, type DiffFile } from '@git-diff-view/core'
import type { Root } from 'hast'

/** Cached adapter instance — reused across calls since the singleton highlighter is shared. */
let cachedAdapter: ReturnType<typeof createDiffHighlighter> | null = null

/** Max lines before skipping syntax highlighting entirely. */
const MAX_LINES = 15000

/**
 * Create the DiffFileHighlighter adapter for @git-diff-view/core.
 *
 * Takes the resolved highlighter instance so `getAST` can work
 * synchronously. Only call after `ensureLanguage()`.
 */
function createDiffHighlighter(h: import('shiki').Highlighter) {
  return {
    name: 'shiki',
    type: 'style' as const,
    maxLineToIgnoreSyntax: MAX_LINES,
    setMaxLineToIgnoreSyntax: () => {},
    ignoreSyntaxHighlightList: [] as (string | RegExp)[],
    setIgnoreSyntaxHighlightList: () => {},

    getAST(raw: string, _fileName?: string, lang?: string): Root {
      const shikiLang = lang ?? 'plaintext'

      if (!h.getLoadedLanguages().includes(shikiLang)) {
        return { type: 'root', children: [] }
      }

      return h.codeToHast(raw, {
        lang: shikiLang,
        theme: SHIKI_THEME_NAME
      })
    },

    processAST(ast: Root) {
      return processAST(ast)
    },

    hasRegisteredCurrentLang(lang: string): boolean {
      return h.getLoadedLanguages().includes(lang)
    }
  }
}

/**
 * Asynchronously apply syntax highlighting to a DiffFile.
 *
 * This is safe to call at any time — if the language can't be loaded,
 * the diff remains plain text. The DiffFile's subscriber is notified
 * when highlighting completes, triggering re-render.
 */
export async function highlightDiffFile(diffFile: DiffFile, lang: string): Promise<void> {
  if (lang === 'plaintext') return

  const loaded = await ensureLanguage(lang)
  if (!loaded) return

  try {
    if (!cachedAdapter) cachedAdapter = createDiffHighlighter(await getHighlighter())
    diffFile.initSyntax({ registerHighlighter: cachedAdapter })
    // Notify subscribers so the UI re-renders with syntax data
    diffFile.notifyAll()
  } catch {
    // Syntax highlighting failed — diff remains usable without it
  }
}
