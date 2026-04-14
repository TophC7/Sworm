/**
 * Lazy Shiki syntax highlighting integration for the diff viewer.
 *
 * Wraps Shiki as a DiffFileHighlighter adapter compatible with
 * @git-diff-view/core's `initSyntax()`. Languages are loaded on
 * demand — the first render shows plain text + char-level diff
 * highlights, then syntax coloring appears asynchronously.
 *
 * Uses a CSS-variables theme so syntax colors derive from the
 * ADE design tokens defined in app.css (--shiki-* custom properties).
 */

import type { Highlighter } from 'shiki'
import { createCssVariablesTheme } from '@shikijs/core'
import { processAST, type DiffFile } from '@git-diff-view/core'
import type { Root } from 'hast'

const THEME = createCssVariablesTheme({
  name: 'ade',
  variablePrefix: '--shiki-',
  variableDefaults: {}
})
const THEME_NAME = 'ade'

/** Shiki singleton — created on first use. */
let highlighterPromise: Promise<Highlighter> | null = null
let highlighterInstance: Highlighter | null = null

/** Track in-flight language loads so concurrent requests await the same promise. */
const langLoadPromises = new Map<string, Promise<boolean>>()

/** Cached adapter instance — reused across calls since the singleton highlighter is shared. */
let cachedAdapter: ReturnType<typeof createDiffHighlighter> | null = null

/** Max lines before skipping syntax highlighting entirely. */
const MAX_LINES = 15000

/**
 * Get or create the singleton Shiki highlighter.
 * Loads with no languages — they're added on demand.
 */
async function getHighlighter(): Promise<Highlighter> {
  if (highlighterInstance) return highlighterInstance

  if (!highlighterPromise) {
    highlighterPromise = import('shiki').then(async ({ createHighlighter }) => {
      const h = await createHighlighter({
        themes: [THEME],
        langs: []
      })
      highlighterInstance = h
      return h
    })
  }

  return highlighterPromise
}

/**
 * Ensure a language grammar is loaded in the Shiki highlighter.
 * Returns true if the language is available after this call.
 * Concurrent calls for the same language share one load promise.
 */
async function ensureLanguage(lang: string): Promise<boolean> {
  if (lang === 'plaintext') return false

  const h = await getHighlighter()
  if (h.getLoadedLanguages().includes(lang)) return true

  // Await an in-flight load if one exists
  const existing = langLoadPromises.get(lang)
  if (existing) return existing

  const promise = (async () => {
    try {
      await h.loadLanguage(lang as Parameters<typeof h.loadLanguage>[0])
      return true
    } catch {
      // Language not supported by Shiki — skip silently
      return false
    } finally {
      langLoadPromises.delete(lang)
    }
  })()

  langLoadPromises.set(lang, promise)
  return promise
}

/**
 * Create the DiffFileHighlighter adapter for @git-diff-view/core.
 *
 * IMPORTANT: Only call this AFTER the language has been loaded via
 * `ensureLanguage()`. The `getAST` method is synchronous and will
 * fail if the grammar isn't already available.
 */
function createDiffHighlighter() {
  return {
    name: 'shiki',
    type: 'style' as const,
    maxLineToIgnoreSyntax: MAX_LINES,
    setMaxLineToIgnoreSyntax: () => {},
    ignoreSyntaxHighlightList: [] as (string | RegExp)[],
    setIgnoreSyntaxHighlightList: () => {},

    getAST(raw: string, _fileName?: string, lang?: string): Root {
      if (!highlighterInstance) {
        return { type: 'root', children: [] }
      }

      const shikiLang = lang ?? 'plaintext'

      if (!highlighterInstance.getLoadedLanguages().includes(shikiLang)) {
        return { type: 'root', children: [] }
      }

      return highlighterInstance.codeToHast(raw, {
        lang: shikiLang,
        theme: THEME_NAME
      })
    },

    processAST(ast: Root) {
      return processAST(ast)
    },

    hasRegisteredCurrentLang(lang: string): boolean {
      if (!highlighterInstance) return false
      return highlighterInstance.getLoadedLanguages().includes(lang)
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
    if (!cachedAdapter) cachedAdapter = createDiffHighlighter()
    diffFile.initSyntax({ registerHighlighter: cachedAdapter })
    // Notify subscribers so the UI re-renders with syntax data
    diffFile.notifyAll()
  } catch {
    // Syntax highlighting failed — diff remains usable without it
  }
}
