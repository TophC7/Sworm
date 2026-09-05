// Minimal xterm wrapper for task PTY runs.
//
// Just mount xterm, pipe output, forward input — no resume tokens, no
// provider-specific behavior.

import { backend } from '$lib/api/backend'
import { RenderBarrier } from '$lib/features/sessions/terminal/renderBarrier'
import { MONO_FONT_FAMILY } from '$lib/fonts'
import type { PtyEvent, TerminalTransferState } from '$lib/types/backend'
import type { TaskRunStatus } from '$lib/features/workbench/model'
import type { Channel } from '@tauri-apps/api/core'
import { FitAddon } from '@xterm/addon-fit'
import { SerializeAddon } from '@xterm/addon-serialize'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { Terminal, type IDisposable, type ITerminalOptions } from '@xterm/xterm'

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

export interface TaskTerminalInit {
  runId: string
  folderPath: string
  taskId: string
  activeFilePath: string | null
  clearBeforeStart?: boolean
  onStatusChange?: (status: TaskRunStatus, exitCode: number | null) => void
}

export class TaskTerminal {
  private readonly term: Terminal
  private readonly fit: FitAddon
  private readonly serializeAddon: SerializeAddon
  private readonly disposers: IDisposable[] = []
  private readonly hostEl: HTMLDivElement
  private resizeObserver: ResizeObserver | null = null
  private container: HTMLElement | null = null
  private runId: string
  private readonly folderPath: string
  private readonly taskId: string
  private readonly activeFilePath: string | null
  private readonly onStatusChange?: (status: TaskRunStatus, exitCode: number | null) => void
  private disposed = false
  private spawned = false
  private status: TaskRunStatus | 'idle' = 'idle'
  private outputChannel: Channel<Uint8Array> | null = null
  private eventsChannel: Channel<PtyEvent> | null = null
  private readonly barrier = new RenderBarrier()

  constructor(init: TaskTerminalInit) {
    this.runId = init.runId
    this.folderPath = init.folderPath
    this.taskId = init.taskId
    this.activeFilePath = init.activeFilePath
    this.onStatusChange = init.onStatusChange

    this.hostEl = document.createElement('div')
    this.hostEl.style.width = '100%'
    this.hostEl.style.height = '100%'

    this.term = new Terminal(TERMINAL_OPTIONS)
    this.fit = new FitAddon()
    this.serializeAddon = new SerializeAddon()
    this.term.loadAddon(this.fit)
    this.term.loadAddon(this.serializeAddon)
    this.term.loadAddon(new WebLinksAddon())
    this.term.open(this.hostEl)

    if (init.clearBeforeStart) this.term.clear()

    this.disposers.push(
      this.term.onData((data) => {
        if (this.status !== 'running') return
        const bytes = textEncoder.encode(data)
        backend.tasks.write(this.runId, bytes).catch(() => {})
      })
    )

    // Tasks rely on native xterm key handling.
  }

  attach(container: HTMLElement): void {
    if (this.disposed) {
      throw new Error(`Task terminal ${this.runId} has been disposed`)
    }

    if (this.container && this.container !== container) {
      this.detach()
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
    this.fitTerminal()

    requestAnimationFrame(() => {
      this.fitTerminal()
    })
  }

  detach(): void {
    this.resizeObserver?.disconnect()
    this.resizeObserver = null

    if (this.hostEl.parentElement) {
      this.hostEl.parentElement.removeChild(this.hostEl)
    }

    this.container = null
  }

  hasStarted(): boolean {
    return this.spawned
  }

  /** Start the PTY. Resolves when the spawn command returns; output
   *  and events then flow over the channels. */
  async start(): Promise<void> {
    if (this.spawned || this.disposed) return
    this.spawned = true
    this.status = 'starting'
    this.barrier.reset()

    const { cols, rows } = this.term
    try {
      await backend.tasks.start(
        this.runId,
        this.folderPath,
        this.taskId,
        this.activeFilePath,
        cols,
        rows,
        (data) => this.handleOutput(data),
        (event) => this.handlePtyEvent(event)
      )
    } catch (error) {
      this.status = 'failed'
      this.onStatusChange?.('failed', null)
      throw error
    }
  }

  private handleOutput(data: Uint8Array): void {
    if (this.disposed) return
    const sequence = this.barrier.next()
    this.term.write(data, () => this.barrier.markRendered(sequence))
  }

  private handlePtyEvent(event: PtyEvent): void {
    if (this.disposed) return
    if (event.run_id !== this.runId) return
    const sequence = this.barrier.next()
    if (event.type === 'started') {
      this.status = 'running'
      this.onStatusChange?.('running', null)
      this.barrier.markRendered(sequence)
    } else if (event.type === 'exit') {
      const code = event.code ?? null
      this.status = 'exited'
      this.onStatusChange?.('exited', code)
      this.barrier.markRendered(sequence)
    } else if (event.type === 'error') {
      this.term.write(textEncoder.encode(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`), () =>
        this.barrier.markRendered(sequence)
      )
      this.status = 'failed'
      this.onStatusChange?.('failed', null)
    } else {
      this.barrier.markRendered(sequence)
    }
  }

  async exportTransferState(): Promise<TerminalTransferState> {
    const inert = this.status === 'exited' || this.status === 'failed'
    const targetSequence = inert ? 0 : await backend.pty.pause(this.runId)
    if (!inert) await this.barrier.waitFor(targetSequence)
    await this.writeAndWait('')
    const buffer = this.term.buffer.active
    return {
      runId: inert ? null : this.runId,
      serializedBuffer: this.serializeAddon.serialize(),
      cols: this.term.cols,
      rows: this.term.rows,
      viewportPosition: buffer.viewportY,
      lastSequence: targetSequence,
      status: this.status
    }
  }

  async importTransferState(state: TerminalTransferState, transferId: string): Promise<void> {
    if (this.disposed) throw new Error(`Task terminal ${this.runId} has been disposed`)
    this.barrier.reset()
    this.status =
      state.status === 'starting' ||
      state.status === 'running' ||
      state.status === 'exited' ||
      state.status === 'failed'
        ? state.status
        : 'running'
    this.term.reset()
    this.term.resize(state.cols, state.rows)
    await this.writeAndWait(state.serializedBuffer)
    this.term.scrollToLine(state.viewportPosition)
    this.spawned = true
    if (state.runId == null) return
    this.runId = state.runId

    const output = backend.tasks.createOutputChannel((data) => this.handleOutput(data))
    const events = backend.tasks.createEventChannel((event) => this.handlePtyEvent(event))
    this.outputChannel = output
    this.eventsChannel = events

    try {
      const seq = await backend.pty.attach(this.runId, transferId, output, events)
      this.barrier.seed(seq)
    } catch (error) {
      this.outputChannel = null
      this.eventsChannel = null
      throw error
    }
  }

  detachForTransfer(): void {
    if (this.disposed) return
    this.disposed = true
    this.outputChannel = null
    this.eventsChannel = null
    this.disposeSurface()
  }

  markPtyLost(): void {
    this.status = 'failed'
    this.onStatusChange?.('failed', null)
  }

  focus(): void {
    this.term.focus()
  }

  /** Stop the PTY without tearing down xterm — allows the user to
   *  keep reading output after the process exits. */
  async stopProcess(): Promise<void> {
    if (this.status !== 'starting' && this.status !== 'running') return
    await backend.tasks.stop(this.runId).catch(() => {})
    this.status = 'exited'
    this.onStatusChange?.('exited', null)
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    // Kill the PTY too; a lingering task shouldn't outlive its tab.
    if (this.status === 'starting' || this.status === 'running') {
      backend.tasks.stop(this.runId).catch(() => {})
    }
    this.outputChannel = null
    this.eventsChannel = null
    this.disposeSurface()
  }

  private disposeSurface(): void {
    this.detach()
    for (const disposer of this.disposers) disposer.dispose()
    this.disposers.length = 0
    this.barrier.reset()
    this.term.dispose()
  }

  private writeAndWait(data: string | Uint8Array): Promise<void> {
    const { promise, resolve } = Promise.withResolvers<void>()
    this.term.write(data, resolve)
    return promise
  }

  private fitTerminal(): void {
    this.fit.fit()
  }

  private fitAndSyncSize(): void {
    this.fitTerminal()
    if (this.status !== 'running') return

    const { cols, rows } = this.term
    backend.tasks.resize(this.runId, cols, rows).catch(() => {})
  }
}
