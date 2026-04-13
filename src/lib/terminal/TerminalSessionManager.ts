import { backend } from '$lib/api/backend'
import { feedOutput, markCompleted } from '$lib/stores/activity.svelte'
import type { PtyEvent, Session } from '$lib/types/backend'
import type { Channel } from '@tauri-apps/api/core'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { Terminal, type IDisposable, type ITerminalOptions } from '@xterm/xterm'

const TERMINAL_OPTIONS: ITerminalOptions = {
  cursorBlink: true,
  fontSize: 13,
  fontFamily: "'Monocraft Nerd Font', monospace",
  scrollback: 10000,
  convertEol: true,
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

const textEncoder = new TextEncoder()

type EventListener = (event: PtyEvent) => void
type ErrorListener = (message: string) => void

export class TerminalSessionManager {
  readonly sessionId: string

  private terminal: Terminal | null = null
  private fitAddon: FitAddon | null = null
  private webLinksAddon: WebLinksAddon | null = null
  private hostEl: HTMLDivElement | null = null
  private container: HTMLElement | null = null
  private resizeObserver: ResizeObserver | null = null
  private inputDisposable: IDisposable | null = null
  private outputChannel: Channel<number[]> | null = null
  private eventsChannel: Channel<PtyEvent> | null = null
  private ptyActive = false
  private inputEnabled = true
  private disposed = false
  private viewportPosition = 0
  private lastError: string | null = null
  private providerId: string | null = null
  private readonly textDecoder = new TextDecoder()
  private readonly eventListeners = new Set<EventListener>()
  private readonly errorListeners = new Set<ErrorListener>()

  constructor(sessionId: string) {
    this.sessionId = sessionId
  }

  isPtyActive(): boolean {
    return this.ptyActive
  }

  isAttached(): boolean {
    return this.container !== null
  }

  getLastError(): string | null {
    return this.lastError
  }

  setInputEnabled(enabled: boolean): void {
    this.inputEnabled = enabled
    if (!enabled) {
      this.terminal?.blur()
    }
  }

  registerEventListener(listener: EventListener): () => void {
    this.eventListeners.add(listener)
    return () => {
      this.eventListeners.delete(listener)
    }
  }

  registerErrorListener(listener: ErrorListener): () => void {
    this.errorListeners.add(listener)
    return () => {
      this.errorListeners.delete(listener)
    }
  }

  async attach(container: HTMLElement): Promise<void> {
    if (this.disposed) {
      throw new Error(`Terminal session ${this.sessionId} has been disposed`)
    }

    await this.ensureTerminal()

    if (this.container && this.container !== container) {
      this.detach()
    }

    if (!this.hostEl || !this.terminal) {
      return
    }

    this.container = container
    if (this.hostEl.parentElement !== container) {
      container.replaceChildren(this.hostEl)
    }

    this.resizeObserver?.disconnect()
    this.resizeObserver = new ResizeObserver(() => {
      this.fitAndSyncSize()
    })
    this.resizeObserver.observe(container)

    requestAnimationFrame(() => {
      this.fitTerminal()
      if (this.viewportPosition > 0) {
        this.terminal?.scrollToLine(this.viewportPosition)
      }
    })
  }

  detach(): void {
    if (!this.terminal) {
      this.container = null
      return
    }

    this.viewportPosition = ((this.terminal as any).buffer?.active?.viewportY as number | undefined) ?? 0
    this.resizeObserver?.disconnect()
    this.resizeObserver = null

    if (this.hostEl?.parentElement) {
      this.hostEl.parentElement.removeChild(this.hostEl)
    }

    this.container = null
  }

  async startPty(session: Session): Promise<void> {
    if (this.disposed || this.ptyActive) {
      return
    }

    await this.ensureTerminal()
    this.releaseChannels()

    const terminal = this.terminal
    if (!terminal) {
      return
    }

    this.lastError = null
    this.providerId = session.provider_id

    const output = backend.sessions.createOutputChannel((data) => {
      const bytes = new Uint8Array(data)
      this.terminal?.write(bytes)
      // Feed output to activity classifier for agent state detection
      feedOutput(this.sessionId, session.provider_id, this.textDecoder.decode(bytes, { stream: true }))
    })
    const events = backend.sessions.createEventChannel((event) => {
      if (event.type === 'started') {
        this.ptyActive = true
        this.lastError = null
      }

      if (event.type === 'exit') {
        this.ptyActive = false
        this.terminal?.write('\r\n\x1b[33m[Process exited]\x1b[0m\r\n')
        this.releaseChannels()
        markCompleted(this.sessionId)
      }

      if (event.type === 'error') {
        this.lastError = event.message ?? 'Unknown PTY error'
        this.emitError(this.lastError)
      }

      this.emitEvent(event)
    })

    this.outputChannel = output
    this.eventsChannel = events

    this.fitAndSyncSize()

    try {
      await backend.sessions.startWithChannels(
        session.id,
        this.terminal?.cols ?? 120,
        this.terminal?.rows ?? 32,
        output,
        events
      )
      this.ptyActive = true
    } catch (error) {
      this.ptyActive = false
      this.releaseChannels()
      this.lastError = String(error)
      this.emitError(this.lastError)
      throw error
    }
  }

  async stopPty(): Promise<void> {
    try {
      await backend.sessions.stop(this.sessionId)
    } finally {
      this.ptyActive = false
      this.releaseChannels()
    }
  }

  dispose(): void {
    if (this.disposed) {
      return
    }

    this.disposed = true
    this.detach()

    if (this.ptyActive) {
      void this.stopPty().catch(() => {})
    } else {
      this.releaseChannels()
    }

    this.inputDisposable?.dispose()
    this.inputDisposable = null
    this.terminal?.dispose()
    this.terminal = null
    this.fitAddon = null
    this.webLinksAddon = null
    this.hostEl = null
    this.eventListeners.clear()
    this.errorListeners.clear()
  }

  private async ensureTerminal(): Promise<void> {
    if (this.terminal) {
      return
    }

    // Wait for the terminal font to load before creating the terminal.
    // xterm.js measures character widths on a canvas at creation time —
    // if the font isn't ready, it measures with the fallback and the
    // custom font never renders correctly.
    const fontSpec = `${TERMINAL_OPTIONS.fontSize ?? 13}px ${TERMINAL_OPTIONS.fontFamily ?? 'monospace'}`
    try {
      await document.fonts.load(fontSpec)
    } catch {
      // Font load can fail if the font name is invalid; proceed with fallback
    }

    this.hostEl = document.createElement('div')
    this.hostEl.className = 'terminal-session-host'
    Object.assign(this.hostEl.style, {
      width: '100%',
      height: '100%'
    })

    this.terminal = new Terminal(TERMINAL_OPTIONS)
    this.fitAddon = new FitAddon()
    this.webLinksAddon = new WebLinksAddon()
    this.terminal.loadAddon(this.fitAddon)
    this.terminal.loadAddon(this.webLinksAddon)
    this.terminal.open(this.hostEl)

    this.inputDisposable = this.terminal.onData((data) => {
      if (!this.ptyActive || !this.inputEnabled) {
        return
      }

      const encoded = textEncoder.encode(data)
      backend.sessions.write(this.sessionId, encoded).catch((error) => {
        console.error('Session write error:', error)
      })
    })
  }

  private fitAndSyncSize(): void {
    this.fitTerminal()
    if (this.ptyActive && this.terminal) {
      void backend.sessions.resize(this.sessionId, this.terminal.cols, this.terminal.rows).catch(() => {})
    }
  }

  private fitTerminal(): void {
    try {
      this.fitAddon?.fit()
    } catch {
      // Ignore fit failures while a container is hidden or mid-layout.
    }
  }

  private releaseChannels(): void {
    this.outputChannel = null
    this.eventsChannel = null
  }

  private emitEvent(event: PtyEvent): void {
    for (const listener of this.eventListeners) {
      listener(event)
    }
  }

  private emitError(message: string): void {
    for (const listener of this.errorListeners) {
      listener(message)
    }
  }
}
