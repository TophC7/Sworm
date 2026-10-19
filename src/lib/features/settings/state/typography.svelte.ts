import { getSettings } from './settings.svelte'

// Bundled font chains; keep in step with the `--font-*` tokens in src/app.css.
const SANS_FALLBACK = "'Lexend', system-ui, -apple-system, 'Segoe UI', sans-serif"
const MONO_FALLBACK = "'Monocraft Nerd Font', 'Monocraft', 'Symbols Nerd Font Mono', 'Symbols Nerd Font', monospace"
const DEFAULT_CODE_FONT_SIZE = 13
/** Monaco reads line heights below 8 as a multiple of the font size: 13px code keeps its 20px lines. */
export const CODE_LINE_HEIGHT = 1.5

/**
 * A chosen family goes ahead of the bundled chain, so a name that is not
 * installed still lands on a bundled face and Nerd Font glyphs keep resolving.
 */
function withFallback(name: string | null | undefined, fallback: string): string {
  const family = name?.trim()
  return family ? `"${family.replace(/["\\]/g, '\\$&')}", ${fallback}` : fallback
}

const typography = $derived(getSettings()?.typography)
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
