import { backend } from '$lib/api/backend'
import type { StreamHandle } from '$lib/api/transport'
import { MONO_FONT_FAMILY } from '$lib/fonts'
import { resolveTerminalKey } from '$lib/features/sessions/terminal/terminalKeymap'
import { TerminalTitleParser } from '$lib/features/sessions/terminal/terminalTitle'
import { RenderBarrier } from '$lib/features/sessions/terminal/renderBarrier'
import type { TabId } from '$lib/features/workbench/model'
import {
  getActiveFolderPath,
  getTabs,
  persistSessionTabRunId,
  setSessionTabResumeToken,
  setSessionTabRunId,
  setSessionTabStatus,
  setSessionTabTitle
} from '$lib/features/workbench/state.svelte'
import type { PtyEvent, SessionSpec, SessionStartInfo, TerminalTransferState } from '$lib/types/backend'
import { platform, requireNative } from '$lib/platform'
import { FitAddon } from '@xterm/addon-fit'
import { ImageAddon } from '@xterm/addon-image'
import { SerializeAddon } from '@xterm/addon-serialize'
import { openLink } from '$lib/features/workbench/links/openLink'
import { TerminalLinkProvider } from '$lib/features/sessions/terminal/TerminalLinkProvider'
import { WebglAddon } from '@xterm/addon-webgl'
import { Terminal, type IDisposable, type ITerminalOptions } from '@xterm/xterm'
import { splitRemotePath } from '$lib/utils/paths'
import { copyToClipboard } from '$lib/utils/clipboard'

const TERMINAL_OPTIONS: ITerminalOptions = {
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

const textEncoder = new TextEncoder()

type EventListener = (event: PtyEvent) => void
type ErrorListener = (message: string) => void
type DeferredOutput = { bytes: Uint8Array; sequence: number }

export class TerminalSessionManager {
  readonly tabId: TabId
  // PTY identity mirrors the persisted tab while the run may still be live.
  private runId: string | null = null
  // Exit clears the restart id, but the runtime stream identity remains for
  // trailing replay and explicit cleanup until its stream is released.
  private streamRunId: string | null = null
  private startingRunId: string | null = null

  private terminal: Terminal | null = null
  private fitAddon: FitAddon | null = null
  private imageAddon: ImageAddon | null = null
  private serializeAddon: SerializeAddon | null = null
  private linkProviderDisposable: IDisposable | null = null
  private webglAddon: WebglAddon | null = null
  private webglContextLossDisposable: IDisposable | null = null
  private hostEl: HTMLDivElement | null = null
  private container: HTMLElement | null = null
  private resizeObserver: ResizeObserver | null = null
  private inputDisposable: IDisposable | null = null
  private oscDisposable: IDisposable | null = null
  private stream: StreamHandle<SessionStartInfo | number> | null = null
  private ptyActive = false
  private inputEnabled = true
  private disposed = false
  private viewportPosition = 0
  private lastError: string | null = null
  private providerId: string | null = null
  private readonly barrier = new RenderBarrier()
  private transferBarrierActive = false
  // Single-flight PTY spawn: concurrent callers (mount debounce, Restart)
  // await the same promise instead of spawning twice.
  private startPromise: Promise<SessionStartInfo> | null = null
  private stopPromise: Promise<void> | null = null
  // Terminal creation yields while the font loads. Every entry path
  // shares this promise so concurrent attach/load/start calls cannot
  // create competing xterm instances or WebGL contexts.
  private terminalPromise: Promise<void> | null = null
  private textDecoder = new TextDecoder()
  private readonly titleParser = new TerminalTitleParser()
  private readonly eventListeners = new Set<EventListener>()
  private readonly errorListeners = new Set<ErrorListener>()
  private reconnecting = false
  private readonly reconnectListeners = new Set<(reconnecting: boolean) => void>()
  private remoteStatusReady: Promise<void> | null = null
  private stopRemoteStatus: (() => void) | null = null

  // Hidden sessions defer xterm parsing until attach/export. Bounded to
  // DEFERRED_BYTES_CAP, dropping oldest; dropped bytes fall outside xterm
  // scrollback in practice.
  private static readonly DEFERRED_BYTES_CAP = 4 * 1024 * 1024
  private deferredOutput: DeferredOutput[] = []
  private deferredByteCount = 0

  constructor(tabId: TabId) {
    this.tabId = tabId
    const folderPath = getTabs().find((tab) => tab.id === tabId)?.folderPath
    if (folderPath && splitRemotePath(folderPath) && platform.native) {
      this.remoteStatusReady = platform.native.remotes
        .onRunStatus((event) => {
          if (this.disposed || event.runId !== this.runId || event.runId !== this.streamRunId) return
          this.setReconnecting(event.state === 'reconnecting')
        })
        .then((stop) => {
          if (this.disposed) stop()
          else this.stopRemoteStatus = stop
        })
        .catch((error) => console.error('Remote run status listener failed:', error))
    }
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

  private getFolderPath(): string | null {
    return getTabs().find((t) => t.id === this.tabId)?.folderPath ?? getActiveFolderPath() ?? null
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

  registerReconnectListener(listener: (reconnecting: boolean) => void): () => void {
    this.reconnectListeners.add(listener)
    listener(this.reconnecting)
    return () => {
      this.reconnectListeners.delete(listener)
    }
  }

  private setReconnecting(reconnecting: boolean): void {
    if (this.reconnecting === reconnecting) return
    this.reconnecting = reconnecting
    for (const listener of this.reconnectListeners) listener(reconnecting)
  }

  async attach(container: HTMLElement): Promise<void> {
    if (this.disposed) {
      throw new Error(`Terminal session ${this.tabId} has been disposed`)
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
    // detach() drops the WebGL context so hidden sessions don't hold
    // GPU resources; bring it back for the visible one.
    if (this.webglAddon === null) {
      this.enableWebglRenderer()
    }

    this.resizeObserver?.disconnect()
    this.resizeObserver = new ResizeObserver(() => {
      this.fitAndSyncSize()
    })
    this.resizeObserver.observe(container)

    // Drain bytes that arrived while we were detached. One concatenated
    // write minimizes xterm parser overhead vs. per-chunk replay; xterm
    // chunks the work internally across frames anyway.
    this.flushDeferredBytes()

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
    this.webglContextLossDisposable?.dispose()
    this.webglContextLossDisposable = null
    this.webglAddon?.dispose()
    this.webglAddon = null

    if (this.hostEl?.parentElement) {
      this.hostEl.parentElement.removeChild(this.hostEl)
    }

    this.container = null
  }

  async startPty(spec: Omit<SessionSpec, 'runId'>): Promise<SessionStartInfo> {
    if (this.disposed || this.ptyActive) {
      return { resumed: false, resumeToken: null }
    }
    if (this.startPromise) {
      return this.startPromise
    }

    const promise = this.spawnPty(spec)
    this.startPromise = promise
    try {
      return await promise
    } finally {
      if (this.startPromise === promise) this.startPromise = null
    }
  }

  private async spawnPty(spec: Omit<SessionSpec, 'runId'>): Promise<SessionStartInfo> {
    this.textDecoder = new TextDecoder()
    this.titleParser.reset()
    await this.ensureTerminal()
    await this.remoteStatusReady
    if (this.streamRunId) {
      await this.stopPty(false)
    } else {
      this.releaseStream()
    }

    const terminal = this.terminal
    if (!terminal) {
      return { resumed: false, resumeToken: null }
    }

    this.lastError = null
    this.providerId = spec.providerId
    const tab = getTabs().find((candidate) => candidate.id === this.tabId)
    const runId = tab?.kind === 'session' && tab.runId ? tab.runId : crypto.randomUUID()
    this.startingRunId = runId
    await persistSessionTabRunId(this.tabId, runId)
    this.runId = runId
    this.barrier.reset()
    this.streamRunId = runId

    const stream = backend.sessions.start({ ...spec, runId }, terminal.cols, terminal.rows, {
      onOutput: (data) => this.handleOutput(runId, spec.providerId, data),
      onEvent: (event) => this.handlePtyEvent(runId, event)
    })
    this.stream = stream
    if (this.disposed) stream.dispose()
    this.fitAndSyncSize()

    try {
      const info = await stream.ready
      if (this.disposed && this.stream === stream) this.releaseStream()
      this.ptyActive = !this.disposed && this.runId === runId
      return info
    } catch (error) {
      this.ptyActive = false
      if (!this.disposed && !this.stopPromise) {
        this.startingRunId = null
        this.runId = null
        this.lastError = String(error)
        this.emitError(this.lastError)
        setSessionTabStatus(this.tabId, 'failed')
      }
      this.releaseStream()
      throw error
    }
  }

  private handleOutput(runId: string, providerId: string | null, bytes: Uint8Array): void {
    if (this.disposed || this.streamRunId !== runId) return

    const sequence = this.barrier.next()
    const text = this.textDecoder.decode(bytes, { stream: true })
    let title = this.titleParser.push(text)
    if (providerId === 'omp' && title?.startsWith('π')) {
      title = title.slice(1).trimStart()
      if (title.startsWith('>')) title = title.slice(1).trimStart()
    }
    if (title) setSessionTabTitle(this.tabId, title)

    this.renderOutput(bytes, sequence)
  }

  private handlePtyEvent(runId: string, event: PtyEvent): void {
    if (this.disposed || this.streamRunId !== runId) return
    if (event.type === 'synced') {
      if (event.run_id === runId && event.sequence !== undefined) {
        this.barrier.seed(event.sequence)
      }
      return
    }

    const sequence = this.barrier.next()
    if (event.run_id !== runId) {
      this.barrier.markRendered(sequence)
      return
    }
    if (event.type === 'started') {
      this.ptyActive = true
      this.setReconnecting(false)
      this.lastError = null
      setSessionTabStatus(this.tabId, 'running')
      this.barrier.markRendered(sequence)
    } else if (event.type === 'exit') {
      this.ptyActive = false
      this.setReconnecting(false)
      this.runId = null
      setSessionTabRunId(this.tabId, null)
      this.renderOutput(textEncoder.encode('\r\n\x1b[33m[Process exited]\x1b[0m\r\n'), sequence)
      setSessionTabStatus(this.tabId, 'exited')
    } else {
      this.barrier.markRendered(sequence)
      if (event.type === 'error') {
        this.lastError = event.message ?? 'Unknown PTY error'
        this.emitError(this.lastError)
        setSessionTabStatus(this.tabId, 'failed')
      } else if (event.type === 'resumeTokenBound' && event.token) {
        setSessionTabResumeToken(this.tabId, event.token)
      }
    }

    this.emitEvent(event)
  }

  async exportTransferState(): Promise<TerminalTransferState> {
    const transfers = requireNative().transfers
    const folderPath = this.getFolderPath()
    const runId = this.runId ?? (folderPath && !splitRemotePath(folderPath) ? this.streamRunId : null)

    await this.ensureTerminal()
    const terminal = this.terminal
    const serializeAddon = this.serializeAddon
    if (!terminal || !serializeAddon) throw new Error(`Terminal session ${this.tabId} is unavailable`)
    if (!runId) {
      this.flushDeferredBytes()
      await this.writeAndWait('')
      return {
        runId: null,
        providerId: this.providerId ?? undefined,
        serializedBuffer: serializeAddon.serialize(),
        cols: terminal.cols,
        rows: terminal.rows,
        viewportPosition: terminal.buffer.active.viewportY,
        lastSequence: 0,
        status: (getTabs().find((tab) => tab.id === this.tabId) as { status?: string } | undefined)?.status
      }
    }

    this.transferBarrierActive = true
    try {
      this.flushDeferredBytes()
      const targetSequence = await transfers.pause(runId)
      await this.barrier.waitFor(targetSequence)
      await this.writeAndWait('')

      const buffer = terminal.buffer.active
      return {
        runId,
        providerId: this.providerId ?? undefined,
        serializedBuffer: serializeAddon.serialize(),
        cols: terminal.cols,
        rows: terminal.rows,
        viewportPosition: buffer.viewportY,
        lastSequence: targetSequence,
        status: (getTabs().find((tab) => tab.id === this.tabId) as { status?: string } | undefined)?.status
      }
    } finally {
      this.transferBarrierActive = false
    }
  }

  async importTransferState(state: TerminalTransferState, transferId: string): Promise<void> {
    if (this.disposed) throw new Error(`Terminal session ${this.tabId} has been disposed`)
    const transfers = requireNative().transfers

    this.releaseStream()
    this.runId = state.status === 'exited' ? null : state.runId
    this.barrier.reset()
    this.providerId = state.providerId ?? null
    await this.ensureTerminal()
    await this.remoteStatusReady

    const terminal = this.terminal
    if (!terminal) throw new Error(`Terminal session ${this.tabId} is unavailable`)
    terminal.reset()
    terminal.resize(state.cols, state.rows)
    await this.writeAndWait(state.serializedBuffer)
    terminal.scrollToLine(state.viewportPosition)
    this.viewportPosition = state.viewportPosition
    const runId = state.runId
    if (runId == null) {
      this.ptyActive = false
      return
    }
    this.streamRunId = runId

    const stream = transfers.attach(runId, transferId, {
      onOutput: (data) => this.handleOutput(runId, this.providerId, data),
      onEvent: (event) => this.handlePtyEvent(runId, event)
    })
    this.stream = stream
    if (this.disposed) stream.dispose()
    this.transferBarrierActive = true
    try {
      const seq = await stream.ready
      if (!this.disposed && this.stream === stream) {
        this.barrier.seed(seq)
        this.ptyActive = this.runId === runId
      }
    } catch (error) {
      if (this.stream === stream) this.releaseStream()
      this.ptyActive = false
      throw error
    } finally {
      this.transferBarrierActive = false
    }
  }

  detachForTransfer(): void {
    if (this.disposed) return
    this.disposed = true
    this.releaseStream()
    this.disposeSurface()
  }

  detachForWindowClose(): void {
    this.detachForTransfer()
  }

  markPtyLost(): void {
    this.ptyActive = false
    this.runId = null
    setSessionTabRunId(this.tabId, null)
    this.releaseStream()
    this.writeTerminal('\r\n\x1b[31m[Connection to process lost]\x1b[0m\r\n')
    setSessionTabStatus(this.tabId, 'failed')
  }

  stopPty(waitForStart = true): Promise<void> {
    if (this.stopPromise) return this.stopPromise
    const pendingStart = waitForStart ? this.startPromise : null
    const startedRunId = this.streamRunId ?? this.runId ?? this.startingRunId
    const stop = (async () => {
      await pendingStart?.catch(() => {})
      const runId = this.streamRunId ?? this.runId ?? this.startingRunId ?? startedRunId
      if (!runId) return
      await backend.sessions.stop(runId)
      this.ptyActive = false
      this.runId = null
      this.startingRunId = null
      setSessionTabRunId(this.tabId, null)
      this.releaseStream()
      setSessionTabStatus(this.tabId, 'exited')
    })()
    this.stopPromise = stop.finally(() => {
      this.stopPromise = null
    })
    return this.stopPromise
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    if (this.startPromise || this.streamRunId || this.runId) void this.stopPty().catch(() => {})
    this.releaseStream()
    this.disposeSurface()
  }

  private disposeSurface(): void {
    this.stopRemoteStatus?.()
    this.stopRemoteStatus = null
    this.setReconnecting(false)
    this.reconnectListeners.clear()
    this.detach()
    this.inputDisposable?.dispose()
    this.inputDisposable = null
    this.oscDisposable?.dispose()
    this.oscDisposable = null
    this.webglContextLossDisposable?.dispose()
    this.webglContextLossDisposable = null
    this.linkProviderDisposable?.dispose()
    this.linkProviderDisposable = null
    this.terminal?.dispose()
    this.terminal = null
    this.fitAddon = null
    this.imageAddon = null
    this.serializeAddon = null
    this.webglAddon = null
    this.hostEl = null
    this.eventListeners.clear()
    this.errorListeners.clear()
    this.deferredOutput = []
    this.deferredByteCount = 0
    this.barrier.reset()
  }

  sendText(text: string): void {
    this.writeToPty(text)
  }

  private renderOutput(bytes: Uint8Array, sequence: number): void {
    if (this.container || this.transferBarrierActive) {
      this.writeTerminal(bytes, sequence)
    } else {
      this.deferDetachedBytes(bytes, sequence)
    }
  }

  private deferDetachedBytes(bytes: Uint8Array, sequence: number): void {
    this.deferredOutput.push({ bytes, sequence })
    this.deferredByteCount += bytes.byteLength
    // Keep the newest chunk even when it alone exceeds the cap; dropped
    // sequences count as rendered so the pause barrier stays consistent.
    while (this.deferredByteCount > TerminalSessionManager.DEFERRED_BYTES_CAP && this.deferredOutput.length > 1) {
      const dropped = this.deferredOutput.shift()!
      this.deferredByteCount -= dropped.bytes.byteLength
      this.barrier.markRendered(dropped.sequence)
    }
  }

  private flushDeferredBytes(): void {
    if (this.deferredOutput.length === 0 || !this.terminal) return
    const output = this.deferredOutput
    const merged = new Uint8Array(this.deferredByteCount)
    let offset = 0
    for (const chunk of output) {
      merged.set(chunk.bytes, offset)
      offset += chunk.bytes.byteLength
    }
    this.deferredOutput = []
    this.deferredByteCount = 0
    this.terminal.write(merged, () => {
      for (const chunk of output) this.barrier.markRendered(chunk.sequence)
    })
  }

  private writeTerminal(data: string | Uint8Array, sequence?: number): void {
    if (!this.terminal) {
      if (sequence !== undefined) this.barrier.markRendered(sequence)
      return
    }
    this.terminal.write(data, sequence === undefined ? undefined : () => this.barrier.markRendered(sequence))
  }

  private writeAndWait(data: string | Uint8Array): Promise<void> {
    const terminal = this.terminal
    if (!terminal) return Promise.resolve()
    const { promise, resolve } = Promise.withResolvers<void>()
    terminal.write(data, resolve)
    return promise
  }

  private enableWebglRenderer(): void {
    if (!this.terminal) return

    try {
      const addon = new WebglAddon()
      this.webglContextLossDisposable = addon.onContextLoss(() => {
        addon.dispose()
        this.webglAddon = null
        this.webglContextLossDisposable?.dispose()
        this.webglContextLossDisposable = null
        console.warn('WebGL terminal renderer lost its context; using DOM renderer.')
      })
      this.terminal.loadAddon(addon)
      this.webglAddon = addon
    } catch (error) {
      this.webglContextLossDisposable?.dispose()
      this.webglContextLossDisposable = null
      this.webglAddon = null
      console.warn('WebGL terminal renderer unavailable; using DOM renderer.', error)
    }
  }

  private async ensureTerminal(): Promise<void> {
    if (this.terminal || this.disposed) return
    if (this.terminalPromise) return this.terminalPromise

    const promise = this.initializeTerminal()
    this.terminalPromise = promise
    try {
      await promise
    } finally {
      if (this.terminalPromise === promise) this.terminalPromise = null
    }
  }

  private async initializeTerminal(): Promise<void> {
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

    // dispose() may run while font loading yields. Never resurrect a
    // manager after its registry owner has released it.
    if (this.disposed || this.terminal) return

    this.hostEl = document.createElement('div')
    this.hostEl.className = 'terminal-session-host'
    Object.assign(this.hostEl.style, {
      width: '100%',
      height: '100%'
    })

    this.terminal = new Terminal({
      ...TERMINAL_OPTIONS,
      linkHandler: {
        activate: (event: MouseEvent, text: string) => {
          if (event.ctrlKey || event.metaKey) {
            void openLink(text, this.getFolderPath())
          }
        },
        hover: (_event: MouseEvent, text: string) => {
          if (this.hostEl) {
            this.hostEl.title = `Ctrl+click to follow: ${text}`
          }
        },
        leave: () => {
          if (this.hostEl) {
            this.hostEl.title = ''
          }
        },
        allowNonHttpProtocols: true
      }
    })
    this.fitAddon = new FitAddon()
    // xterm core does not render SIXEL / iTerm / Kitty image payloads; it needs the parser addon.
    this.imageAddon = new ImageAddon()
    this.serializeAddon = new SerializeAddon()
    this.terminal.loadAddon(this.fitAddon)
    this.terminal.loadAddon(this.imageAddon)
    this.terminal.loadAddon(this.serializeAddon)
    this.linkProviderDisposable = this.terminal.registerLinkProvider(
      new TerminalLinkProvider(
        this.terminal,
        () => this.getFolderPath(),
        () => this.hostEl
      )
    )
    this.terminal.open(this.hostEl)
    this.enableWebglRenderer()

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
            copyToClipboard(text).catch((err) => {
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

    // Handle OSC 52 clipboard sequences from TUI apps (helix, neovim, …).
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
        copyToClipboard(text).catch((err) => {
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
      text = (await platform.clipboard.readText()) ?? ''
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
    if (!this.ptyActive || !this.inputEnabled || !this.runId) return
    const encoded = textEncoder.encode(data)
    backend.sessions.write(this.runId, encoded).catch((error) => {
      console.error('Session write error:', error)
    })
  }

  private fitAndSyncSize(): void {
    this.fitTerminal()
    if (this.ptyActive && this.terminal && this.runId) {
      void backend.sessions.resize(this.runId, this.terminal.cols, this.terminal.rows).catch(() => {})
    }
  }

  private fitTerminal(): void {
    try {
      this.fitAddon?.fit()
    } catch {
      // Ignore fit failures while a container is hidden or mid-layout.
    }
  }

  private releaseStream(): void {
    this.setReconnecting(false)
    this.stream?.dispose()
    this.stream = null
    this.streamRunId = null
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
