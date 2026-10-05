import type { Root } from 'hast'
import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkGfm from 'remark-gfm'
import { remarkAlert } from 'remark-github-blockquote-alert'
import remarkRehype from 'remark-rehype'
import rehypeRaw from 'rehype-raw'
import rehypeSlug from 'rehype-slug'
import rehypeSanitize, { defaultSchema, type Options } from 'rehype-sanitize'
import rehypeShikiFromHighlighter from '@shikijs/rehype/core'
import rehypeStringify from 'rehype-stringify'
import { visit } from 'unist-util-visit'
import type { Highlighter } from 'shiki'
import { platform } from '$lib/platform'
import { markdownImageSrc } from '$lib/utils/mediaAssets'

const SHIKI_THEME_NAME = 'sworm'
let highlighterPromise: Promise<Highlighter> | null = null

function getHighlighter(): Promise<Highlighter> {
  return (highlighterPromise ??= import('shiki').then(
    ({ createHighlighter, createCssVariablesTheme, createJavaScriptRegexEngine }) =>
      createHighlighter({
        themes: [createCssVariablesTheme({ name: SHIKI_THEME_NAME, variablePrefix: '--shiki-' })],
        langs: [],
        // Match Monaco's engine; avoid the desktop runtime's crashing Oniguruma wasm path.
        engine: createJavaScriptRegexEngine({ forgiving: true })
      })
  ))
}

const markdownSchema: Options = {
  ...defaultSchema,
  tagNames: [...(defaultSchema.tagNames ?? []), 'svg', 'path'],
  attributes: {
    ...defaultSchema.attributes,
    div: [...(defaultSchema.attributes?.div ?? []), ['className', /^markdown-alert(?:-\w+)?$/]],
    p: [['className', 'markdown-alert-title']],
    svg: [['className', 'octicon'], 'viewBox', 'width', 'height', 'ariaHidden'],
    path: ['d']
  },
  protocols: {
    ...defaultSchema.protocols,
    // App links are dispatched through openLink, never browser navigation.
    href: [
      'http',
      'https',
      'mailto',
      'file',
      'issue',
      'pr',
      'omp',
      'skill',
      'rule',
      'local',
      'artifact',
      'history',
      'agent'
    ],
    // Embedded images are MIME-checked after sanitization; data links stay blocked.
    src: [...(defaultSchema.protocols?.src ?? []), 'data']
  }
}

const webMarkdownSchema: Options = {
  ...markdownSchema,
  protocols: {
    ...markdownSchema.protocols,
    src: [...(markdownSchema.protocols?.src ?? []), 'file']
  }
}

export async function renderMarkdown(
  source: string,
  folderPath?: string,
  filePath?: string | null,
  loadLocalImage?: (filePath: string) => Promise<string | null>
): Promise<{ html: string; images: Promise<string | null>[] }> {
  // Web admits file: image URLs only to replace them with text; desktop retains its original sanitizer.
  const schema = 'url' in platform.assets ? markdownSchema : webMarkdownSchema
  const images: Promise<string | null>[] = []
  const processor = unified()
    .use(remarkParse)
    .use(remarkGfm)
    .use(remarkAlert)
    // Sanitize owns the single user-content- prefix, including footnote IDs.
    .use(remarkRehype, { allowDangerousHtml: true, clobberPrefix: '' })
    .use(rehypeRaw)
    .use(rehypeSlug)
    // Sanitize raw HTML and generated IDs before trusted asset/Shiki transforms.
    .use(rehypeSanitize, schema)
    .use(() => async (tree: Root, file) => {
      let needsHighlighting = false
      visit(tree, 'element', (node) => {
        if (node.tagName === 'img' && typeof node.properties.src === 'string') {
          const src = node.properties.src.trim()
          if (/^data:/i.test(src.replace(/[\t\n\r]/g, '')) && !/^data:image\/[a-z0-9.+-]+(?:;[^,]*)?,/i.test(src)) {
            delete node.properties.src
          } else {
            // Trusted URLs are applied after publication; image I/O must not block the document.
            node.properties.dataSwormImage = String(images.length)
            node.properties.ariaBusy = 'true'
            delete node.properties.src
            images.push(markdownImageSrc(src, folderPath, filePath, loadLocalImage).catch(() => null))
          }
        }
        if (node.tagName === 'pre') {
          const code = node.children[0]
          if (
            code?.type === 'element' &&
            code.tagName === 'code' &&
            Array.isArray(code.properties.className) &&
            code.properties.className.some((name) => String(name).startsWith('language-'))
          ) {
            needsHighlighting = true
          }
        }
      })
      // Highlighting is optional: retain sanitized plaintext if initialization or a grammar fails.
      if (needsHighlighting) {
        try {
          const highlighter = await getHighlighter()
          await unified()
            .use(rehypeShikiFromHighlighter, highlighter, {
              theme: SHIKI_THEME_NAME,
              lazy: true,
              fallbackLanguage: 'text',
              onError: (cause) => console.warn('Markdown syntax highlighting failed', cause)
            })
            .run(tree, file)
        } catch (cause) {
          console.warn('Markdown syntax highlighting failed', cause)
        }
      }
    })
    .use(rehypeStringify)

  return { html: String(await processor.process(source)), images }
}
