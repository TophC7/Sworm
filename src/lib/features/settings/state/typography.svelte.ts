import { getDesktopSettings } from './settings.svelte'

// Bundled font chains; keep in step with the `--font-*` tokens in src/app.css.
const SANS_FALLBACK = "'Lexend', system-ui, -apple-system, 'Segoe UI', sans-serif"
const MONO_FALLBACK = "'Monocraft Nerd Font', 'Monocraft', 'Symbols Nerd Font Mono', 'Symbols Nerd Font', monospace"
const DEFAULT_CODE_FONT_SIZE = 13
/** Monaco reads line heights below 8 as a multiple of the font size: 13px code keeps its 20px lines. */
export const CODE_LINE_HEIGHT = 1.5
/** Font size bounds; mirror the 6–72 schemars range on `TypographySettings`. */
export const MIN_FONT_SIZE = 6
export const MAX_FONT_SIZE = 72

// CSS Fonts 4 generic families: quoting turns them into literal (missing) names.
const GENERIC_FAMILIES = new Set([
  'serif',
  'sans-serif',
  'monospace',
  'cursive',
  'fantasy',
  'system-ui',
  'ui-serif',
  'ui-sans-serif',
  'ui-monospace',
  'ui-rounded',
  'math',
  'emoji',
  'fangsong'
])

/**
 * A chosen family goes ahead of the bundled chain, so a name that is not
 * installed still lands on a bundled face and Nerd Font glyphs keep resolving.
 * Generic family keywords pass through unquoted.
 */
function withFallback(name: string | null | undefined, fallback: string): string {
  const family = name?.trim()
  if (!family) return fallback
  const generic = family.toLowerCase()
  return `${GENERIC_FAMILIES.has(generic) ? generic : `"${family.replace(/["\\]/g, '\\$&')}"`}, ${fallback}`
}

const typography = $derived(getDesktopSettings()?.typography)
const uiFontFamily = $derived(withFallback(typography?.ui_font_family, SANS_FALLBACK))
const codeFontFamily = $derived(withFallback(typography?.code_font_family, MONO_FALLBACK))
const codeFontSize = $derived(typography?.code_font_size ?? DEFAULT_CODE_FONT_SIZE)
const terminalFontFamily = $derived(
  withFallback(typography?.terminal_font_family?.trim() || typography?.code_font_family, MONO_FALLBACK)
)
const terminalFontSize = $derived(typography?.terminal_font_size ?? codeFontSize)

export const getUiFontFamily = () => uiFontFamily
export const getCodeFontFamily = () => codeFontFamily
export const getCodeFontSize = () => codeFontSize
export const getTerminalFontFamily = () => terminalFontFamily
export const getTerminalFontSize = () => terminalFontSize
