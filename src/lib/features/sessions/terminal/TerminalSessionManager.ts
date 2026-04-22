import { backend } from '$lib/api/backend'
import { feedOutput, markCompleted } from '$lib/features/sessions/state/sessionActivity.svelte'
import { resolveTerminalKey } from '$lib/features/sessions/terminal/terminalKeymap'
import type { PtyEvent, Session } from '$lib/types/backend'
import type { Channel } from '@tauri-apps/api/core'
import { readText } from '@tauri-apps/plugin-clipboard-manager'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { Terminal, type IDisposable, type ITerminalOptions } from '@xterm/xterm'

const TERMINAL_OPTIONS: ITerminalOptions = {
  cursorBlink: true,
  fontSize: 13,
  fontFamily: "'Monocraft Nerd Font', monospace",
  scrollback: 10000,
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

const textEncoder = new TextEncoder()

function decodeBase64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i)
  }
  return bytes
}

type EventListener = (event: PtyEvent) => void
type ErrorListener = (message: string) => void

/**
 * Whether a terminal is currently bound to a live PTY in the backend
 * process, or only displaying historical transcript (input disabled).
 */
export type TerminalMode = 'historical' | 'live'

export class TerminalSessionManager {
  readonly sessionId: string

  private terminal: Terminal | null = null
  private fitAddon: FitAddon | null = null
  private webLinksAddon: WebLinksAddon | null = null
  private hostEl: HTMLDivElement | null = null
  private container: HTMLElement | null = null
  private resizeObserver: ResizeObserver | null = null
  private inputDisposable: IDisposable | null = null
  private oscDisposable: IDisposable | null = null
  private outputChannel: Channel<number[]> | null = null
  private eventsChannel: Channel<PtyEvent> | null = null
  private ptyActive = false
  private inputEnabled = true
  private disposed = false
  private viewportPosition = 0
  private lastError: string | null = null
  private providerId: string | null = null
  private transcriptLoaded = false
  private transcriptByteCount = 0
  // Dedup concurrent attach() calls so we fetch the transcript once.
  private transcriptPromise: Promise<void> | null = null
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

  getMode(): TerminalMode {
    return this.ptyActive ? 'live' : 'historical'
  }

  hasTranscriptLoaded(): boolean {
    return this.transcriptLoaded
  }

  /**
   * Whether the loaded transcript contained any bytes. Used by the
   * mount path to decide whether a session is genuinely "fresh" (and
   * should auto-spawn) or has prior history (and should be treated
   * as historical until the user explicitly resumes).
   */
  hasHistory(): boolean {
    return this.transcriptByteCount > 0
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

  /**
   * Give DOM focus to xterm's hidden textarea. Called after transient
   * modals close so keys like Shift+Tab reach the PTY instead of
   * triggering browser focus-navigation from a stale body focus.
   */
  focus(): void {
    if (!this.inputEnabled) return
    this.terminal?.focus()
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

  /**
   * Replay the persisted transcript into xterm exactly once per
   * manager lifetime. Awaits xterm's write-callback so subsequent
   * live-attach output is guaranteed to land after the history.
   *
   * Safe to call multiple times — concurrent calls dedupe on a
   * single in-flight promise, and after success this is a no-op.
   */
  async loadTranscript(): Promise<void> {
    if (this.transcriptLoaded || this.disposed) {
      return
    }
    if (this.transcriptPromise) {
      return this.transcriptPromise
    }

    this.transcriptPromise = (async () => {
      await this.ensureTerminal()
      const terminal = this.terminal
      if (!terminal) return

      let b64: string
      try {
        b64 = await backend.sessions.getTranscript(this.sessionId)
      } catch (error) {
        // Log but do not throw — a missing transcript is not fatal.
        // This typically happens for sessions that pre-date the
        // transcript table or were freshly reset.
        console.warn(`Transcript fetch failed for ${this.sessionId}:`, error)
        this.transcriptLoaded = true
        return
      }

      if (b64.length > 0) {
        const bytes = decodeBase64ToBytes(b64)
        this.transcriptByteCount = bytes.byteLength
        await new Promise<void>((resolve) => {
          terminal.write(bytes, () => resolve())
        })
      }
      this.transcriptLoaded = true
    })()

    try {
      await this.transcriptPromise
    } finally {
      this.transcriptPromise = null
    }
  }

  /**
   * Reattach to a live PTY that already exists in the backend.
   *
   * Used after a webview reload where the backend process kept
   * running. Ordering matters: callers must `loadTranscript()` first
   * so the historical write completes before live bytes start
   * streaming, otherwise we'd get reordered output.
   */
  async attachLive(session: Session): Promise<boolean> {
    if (this.disposed || this.ptyActive) {
      return this.ptyActive
    }

    let alive = false
    try {
      alive = await backend.sessions.isAlive(session.id)
    } catch (error) {
      console.warn(`isAlive failed for ${session.id}:`, error)
      return false
    }
    if (!alive) {
      return false
    }

    await this.startPty(session)
    return true
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
      // After a successful startPty the PTY is the live source of
      // truth; transcript replay (if any) already happened, so flag
      // it loaded to prevent double-rendering on later attach.
      this.transcriptLoaded = true
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
    this.oscDisposable?.dispose()
    this.oscDisposable = null
    this.terminal?.dispose()
    this.terminal = null
    this.fitAddon = null
    this.webLinksAddon = null
    this.hostEl = null
    this.eventListeners.clear()
    this.errorListeners.clear()
  }

  sendText(text: string): void {
    this.writeToPty(text)
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

    // All key-by-key policy lives in terminalKeymap.ts. This handler
    // is a dumb dispatcher over the resolved KeyAction so adding a new
    // provider quirk or paste mode is a one-file change.
    this.terminal.attachCustomKeyEventHandler((ev) => {
      const action = resolveTerminalKey(ev, this.providerId)
      switch (action.kind) {
        case 'pass':
          return true
        case 'browser':
          return false
        case 'send-pty':
          ev.preventDefault()
          this.writeToPty(action.bytes)
          return false
        case 'paste-text-or-image':
          ev.preventDefault()
          void this.handlePasteKey(false)
          return false
        case 'paste-text-only':
          ev.preventDefault()
          void this.handlePasteKey(true)
          return false
        case 'copy-or-sigint':
          // Only the manager can see the live selection, so the decision
          // lives here while the policy (which key + when) stays in the
          // keymap. With a selection, copy to OS clipboard; without, let
          // xterm send SIGINT like a real terminal.
          if (this.terminal?.hasSelection()) {
            const text = this.terminal.getSelection()
            navigator.clipboard.writeText(text).catch((err) => {
              console.warn('Ctrl+C copy failed:', err)
            })
            this.terminal.clearSelection()
            ev.preventDefault()
            return false
          }
          return true
        default: {
          // Forces a compile error if a new KeyAction kind is added
          // to terminalKeymap.ts without a case here. Default returns
          // true (pass-through) so a forgotten variant degrades to
          // xterm's native handling rather than silently swallowing
          // the keypress.
          const _exhaustive: never = action
          void _exhaustive
          return true
        }
      }
    })

    // Handle OSC 52 clipboard sequences from TUI apps (Fresh, helix, neovim).
    // Format: \x1b]52;<target>;<base64-data>\x07
    this.oscDisposable = this.terminal.parser.registerOscHandler(52, (data) => {
      const sepIdx = data.indexOf(';')
      if (sepIdx === -1) return false
      const payload = data.slice(sepIdx + 1)
      if (payload === '?') return false // clipboard query — not supported
      try {
        const raw = atob(payload)
        const bytes = Uint8Array.from(raw, (c) => c.charCodeAt(0))
        const text = new TextDecoder().decode(bytes)
        navigator.clipboard.writeText(text).catch((err) => {
          console.error('OSC 52 clipboard write failed:', err)
        })
      } catch {
        // invalid base64
      }
      return true
    })

    this.inputDisposable = this.terminal.onData((data) => {
      this.writeToPty(data)
    })
  }

  /**
   * Handle Ctrl+V or Ctrl+Shift+V: read the OS clipboard and deliver
   * text as a bracketed paste. If the clipboard has no text (e.g.
   * image-only), emit the kitty-mode Ctrl+V sequence so image-aware
   * agents (Claude Code, Codex) can still attach the image.
   *
   * Ctrl+Shift+V always ends after the text-paste attempt — the
   * Shift modifier signals "terminal paste" and must not fall back
   * into the agent's image-paste path, which would surprise users
   * expecting classic terminal semantics.
   */
  private async handlePasteKey(shiftHeld: boolean): Promise<void> {
    let text = ''
    try {
      text = (await readText()) ?? ''
    } catch (error) {
      console.warn('clipboard read failed:', error)
    }

    if (text.length > 0) {
      // Strip any embedded paste-end sequence so a hostile clipboard
      // payload can't break out of the bracketed-paste envelope and
      // inject commands into the agent's interpreter. Real terminals
      // do the same filtering.
      const safe = text.replaceAll('\x1b[201~', '')
      this.writeToPty(`\x1b[200~${safe}\x1b[201~`)
      return
    }

    if (shiftHeld) return

    // No text on the clipboard — let image-aware TUIs grab whatever
    // is there by forwarding the kitty-encoded Ctrl+V byte sequence.
    this.writeToPty('\x1b[118;5u')
  }

  private writeToPty(data: string): void {
    if (!this.ptyActive || !this.inputEnabled) return
    const encoded = textEncoder.encode(data)
    backend.sessions.write(this.sessionId, encoded).catch((error) => {
      console.error('Session write error:', error)
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
