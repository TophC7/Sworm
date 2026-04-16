// Sworm Monaco theme — maps the warm base16 palette from app.css
// to Monaco's editor and token color system.
//
// Token-to-color mappings are defined once in TOKEN_PALETTE and derived
// into both Shiki (TextMate scope) and Monaco (Monarch + TextMate rule)
// formats so the two stay in sync automatically.

import type * as Monaco from 'monaco-editor'
import type { ThemeRegistration } from 'shiki'

export const SWORM_THEME_NAME = 'sworm'

// ── Token palette (single source of truth) ──────────────────────────
// Each entry maps TextMate scopes (used by both Shiki and Monaco) to a
// color. `monarchExtras` lists additional Monarch-only token names that
// don't exist in TextMate but should resolve to the same color.

interface TokenMapping {
  scopes: string[]
  monarchExtras?: string[]
  foreground?: string
  fontStyle?: string
}

const TOKEN_PALETTE: TokenMapping[] = [
  // Comments
  { scopes: ['comment', 'punctuation.definition.comment'], foreground: '6e6864', fontStyle: 'italic' },

  // Keywords
  {
    scopes: ['keyword', 'keyword.control', 'keyword.other', 'storage', 'storage.type', 'storage.modifier'],
    foreground: 'd6acff'
  },

  // Strings
  { scopes: ['string', 'string.quoted', 'string.template', 'string.interpolated'], foreground: '98ff7f' },
  {
    scopes: ['string.regexp', 'constant.character.escape'],
    monarchExtras: ['string.escape', 'regexp'],
    foreground: 'b7ffa5'
  },

  // Constants / numbers
  {
    scopes: ['constant.numeric', 'constant.language', 'constant.other', 'support.constant'],
    monarchExtras: ['number', 'constant'],
    foreground: 'ffae57'
  },

  // Functions
  { scopes: ['entity.name.function', 'support.function', 'meta.function-call'], foreground: '7fbaff' },

  // Types / classes
  {
    scopes: ['entity.name.type', 'support.type', 'entity.name.class', 'support.class', 'entity.other.inherited-class'],
    monarchExtras: ['type', 'type.identifier'],
    foreground: 'ffc0ad'
  },

  // Variables / parameters
  { scopes: ['variable', 'variable.other'], foreground: 'e2e2e2' },
  { scopes: ['variable.parameter', 'meta.parameter'], monarchExtras: ['parameter'], foreground: 'ffddd3' },

  // Punctuation / operators
  {
    scopes: ['punctuation', 'meta.brace', 'keyword.operator', 'keyword.operator.assignment'],
    monarchExtras: ['delimiter', 'delimiter.bracket', 'operator'],
    foreground: 'a59c99'
  },

  // Tags (HTML/XML)
  { scopes: ['entity.name.tag'], monarchExtras: ['tag'], foreground: 'ff7672' },
  {
    scopes: ['entity.other.attribute-name'],
    monarchExtras: ['tag.attribute.name', 'attribute.name'],
    foreground: 'ffae57'
  },
  { scopes: [], monarchExtras: ['attribute.value'], foreground: '98ff7f' },

  // Markdown
  { scopes: ['markup.heading'], foreground: 'fff3ef', fontStyle: 'bold' },
  { scopes: ['markup.bold'], fontStyle: 'bold' },
  { scopes: ['markup.italic'], fontStyle: 'italic' },
  { scopes: ['markup.underline.link', 'string.other.link'], foreground: '9effef' },

  // JSON
  {
    scopes: ['support.type.property-name.json', 'support.type.property-name'],
    monarchExtras: ['string.key.json'],
    foreground: '7fbaff'
  },
  { scopes: [], monarchExtras: ['string.value.json'], foreground: '98ff7f' },

  // CSS
  {
    scopes: ['support.type.property-name.css', 'support.type.vendored.property-name'],
    monarchExtras: ['attribute.name.css'],
    foreground: '7fbaff'
  },
  {
    scopes: ['constant.numeric.css', 'keyword.other.unit'],
    monarchExtras: ['attribute.value.number.css', 'attribute.value.unit.css'],
    foreground: 'ffae57'
  },
  { scopes: [], monarchExtras: ['attribute.value.css'], foreground: '98ff7f' }
]

// ── Derive Shiki tokenColors from palette ───────────────────────────

function toShikiTokenColors(): ThemeRegistration['tokenColors'] {
  return TOKEN_PALETTE.filter((m) => m.scopes.length > 0).map((m) => ({
    scope: m.scopes,
    settings: {
      ...(m.foreground ? { foreground: `#${m.foreground}` } : {}),
      ...(m.fontStyle ? { fontStyle: m.fontStyle } : {})
    }
  }))
}

// ── Derive Monaco rules from palette ────────────────────────────────

function toMonacoRules(): Monaco.editor.ITokenThemeRule[] {
  const rules: Monaco.editor.ITokenThemeRule[] = [{ token: '', foreground: 'e2e2e2', background: '131313' }]
  for (const m of TOKEN_PALETTE) {
    const tokens = [...m.scopes, ...(m.monarchExtras ?? [])]
    for (const token of tokens) {
      rules.push({
        token,
        ...(m.foreground ? { foreground: m.foreground } : {}),
        ...(m.fontStyle ? { fontStyle: m.fontStyle } : {})
      })
    }
  }
  return rules
}

// ── Editor chrome colors ────────────────────────────────────────────

const EDITOR_COLORS: Record<string, string> = {
  'editor.background': '#131313',
  'editor.foreground': '#e2e2e2',
  'editor.lineHighlightBackground': '#1c191780',
  'editor.selectionBackground': '#7c2d1560',
  'editor.inactiveSelectionBackground': '#7c2d1530',
  'editor.selectionHighlightBackground': '#7c2d1520',
  'editorCursor.foreground': '#ffb59f',

  'editorLineNumber.foreground': '#6e6864',
  'editorLineNumber.activeForeground': '#a59c99',

  'editorGutter.background': '#131313',

  'editorIndentGuide.background': '#3a322e40',
  'editorIndentGuide.activeBackground': '#3a322e80',

  'editorBracketMatch.background': '#7c2d1530',
  'editorBracketMatch.border': '#ffb59f60',

  'editorWidget.background': '#262220',
  'editorWidget.border': '#3a322e',
  'editorSuggestWidget.background': '#262220',
  'editorSuggestWidget.border': '#3a322e',
  'editorSuggestWidget.selectedBackground': '#3a322e',
  'editorSuggestWidget.highlightForeground': '#ffb59f',
  'editorHoverWidget.background': '#262220',
  'editorHoverWidget.border': '#3a322e',

  'editor.findMatchBackground': '#7c2d1540',
  'editor.findMatchHighlightBackground': '#7c2d1520',

  'scrollbar.shadow': '#00000000',
  'scrollbarSlider.background': '#3a322e80',
  'scrollbarSlider.hoverBackground': '#4a403a80',
  'scrollbarSlider.activeBackground': '#4a403aa0',

  'minimap.background': '#131313',

  'editorOverviewRuler.border': '#00000000',
  focusBorder: '#ffb59f40'
}

// ── Exports ─────────────────────────────────────────────────────────

/** Shiki theme for the highlighter (TextMate tokenColors + editor chrome). */
export const SWORM_SHIKI_THEME: ThemeRegistration = {
  name: SWORM_THEME_NAME,
  type: 'dark',
  colors: EDITOR_COLORS,
  tokenColors: toShikiTokenColors()
}

/** Register the Monaco theme (Monarch + TextMate rules + editor chrome). */
export function registerSwormTheme(monaco: typeof Monaco) {
  monaco.editor.defineTheme(SWORM_THEME_NAME, {
    base: 'vs-dark',
    inherit: false,
    rules: toMonacoRules(),
    colors: EDITOR_COLORS
  })
}
