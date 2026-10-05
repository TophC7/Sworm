import { MONO_FONT_FAMILY } from '$lib/fonts'
import type { ITerminalOptions, Terminal } from '@xterm/xterm'

export const TERMINAL_OPTIONS: ITerminalOptions = {
  cursorBlink: true,
  fontSize: 13,
  fontFamily: MONO_FONT_FAMILY,
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

export function writeAndWait(term: Terminal, data: string | Uint8Array): Promise<void> {
  const { promise, resolve } = Promise.withResolvers<void>()
  term.write(data, resolve)
  return promise
}
