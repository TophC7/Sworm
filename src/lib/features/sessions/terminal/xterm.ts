import { getTerminalFontFamily, getTerminalFontSize } from '$lib/features/settings/state/typography.svelte'
import type { IDisposable, ITerminalOptions, Terminal } from '@xterm/xterm'
import { getActiveTab } from '$lib/features/workbench/state.svelte'
import type { TabId } from '$lib/features/workbench/model'
import { isAnyModalOpen } from '$lib/utils/modalRegistry.svelte'

const BASE_OPTIONS: ITerminalOptions = {
  cursorBlink: true,
  scrollback: 3000,
  convertEol: true,
  vtExtensions: { kittyKeyboard: true },
  theme: {
    background: '#131313',
    foreground: '#e2e2e2',
    cursor: '#ffb59f',
    cursorAccent: '#131313',
    selectionBackground: '#7c2d15',
    selectionForeground: '#e2e2e2',
    black: '#131313',
    red: '#ff7672',
    green: '#98ff7f',
    yellow: '#ffe572',
    blue: '#f29d84',
    magenta: '#763724',
    cyan: '#ffb59f',
    white: '#fff3ef',
    brightBlack: '#a59c99',
    brightRed: '#ffa29f',
    brightGreen: '#b7ffa5',
    brightYellow: '#ffeea5',
    brightBlue: '#ffc0ad',
    brightMagenta: '#ffcbbb',
    brightCyan: '#ffddd3',
    brightWhite: '#fffaf8'
  }
}

/** Options for a new terminal on the current settings font; await `loadTerminalFont()` first. */
export function terminalOptions(): ITerminalOptions {
  return { ...BASE_OPTIONS, fontFamily: getTerminalFontFamily(), fontSize: getTerminalFontSize() }
}

/**
 * xterm measures its cell size when the font changes; measuring before the
 * face loads keeps the fallback's metrics. Unknown names reject, and xterm
 * then renders with the fallback chain.
 */
export async function loadTerminalFont(family = getTerminalFontFamily(), size = getTerminalFontSize()): Promise<void> {
  await document.fonts.load(`${size}px ${family}`).catch(() => {})
}

const liveTerminals = new Map<Terminal, () => void>()
let fontRequest = 0

/** Keeps `term` on the settings font; `refit` resyncs cols/rows after a remeasure. */
export function followTerminalFont(term: Terminal, refit: () => void): IDisposable {
  liveTerminals.set(term, refit)
  return { dispose: () => liveTerminals.delete(term) }
}

/** Moves every live terminal to a font once its face has loaded; a newer call supersedes it. */
export async function applyTerminalFont(family: string, size: number): Promise<void> {
  const request = ++fontRequest
  await loadTerminalFont(family, size)
  if (request !== fontRequest) return
  for (const [term, refit] of liveTerminals) {
    term.options.fontFamily = family
    term.options.fontSize = size
    refit()
  }
}

/** Recheck at the actual focus call: attach/start and animation frames can outlive modal/tab changes. */
export function focusTerminal(term: Terminal | null, tabId: TabId): void {
  if (isAnyModalOpen() || getActiveTab()?.id !== tabId) return
  term?.focus()
}

export function writeAndWait(term: Terminal, data: string | Uint8Array): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>()
  term.write(data, resolve)
  return promise
}
