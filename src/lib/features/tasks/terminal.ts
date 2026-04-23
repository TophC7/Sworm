// Minimal xterm wrapper for task PTY runs.
//
// Deliberately simpler than TerminalSessionManager: tasks have no
// transcript persistence, no resume tokens, no provider-specific
// behavior. Just mount xterm, pipe output, forward input.

import { backend } from '$lib/api/backend'
import type { PtyEvent } from '$lib/types/backend'
import type { TaskRunStatus } from '$lib/features/workbench/model'
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

export interface TaskTerminalInit {
  runId: string
  projectId: string
  taskId: string
  activeFilePath: string | null
  clearBeforeStart?: boolean
  onStatusChange?: (status: TaskRunStatus, exitCode: number | null) => void
}

export class TaskTerminal {
  private readonly term: Terminal
  private readonly fit: FitAddon
  private readonly disposers: IDisposable[] = []
  private readonly hostEl: HTMLDivElement
  private resizeObserver: ResizeObserver | null = null
  private container: HTMLElement | null = null
  private readonly runId: string
  private readonly projectId: string
  private readonly taskId: string
  private readonly activeFilePath: string | null
  private readonly onStatusChange?: (status: TaskRunStatus, exitCode: number | null) => void
  private disposed = false
  private spawned = false
  private status: TaskRunStatus | 'idle' = 'idle'

  constructor(init: TaskTerminalInit) {
    this.runId = init.runId
    this.projectId = init.projectId
    this.taskId = init.taskId
    this.activeFilePath = init.activeFilePath
    this.onStatusChange = init.onStatusChange

    this.hostEl = document.createElement('div')
    this.hostEl.style.width = '100%'
    this.hostEl.style.height = '100%'

    this.term = new Terminal(TERMINAL_OPTIONS)
    this.fit = new FitAddon()
    this.term.loadAddon(this.fit)
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

    // Tasks rely on native xterm key handling. Provider-specific
    // quirks (Shift+Tab handling, Claude/Codex paste variants) live
    // in the session keymap and don't apply here.
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

    const { cols, rows } = this.term
    try {
      await backend.tasks.start(
        this.runId,
        this.projectId,
        this.taskId,
        this.activeFilePath,
        cols,
        rows,
        (data) => {
          this.term.write(new Uint8Array(data))
        },
        (event) => this.handlePtyEvent(event)
      )
    } catch (error) {
      this.status = 'failed'
      this.onStatusChange?.('failed', null)
      throw error
    }
  }

  private handlePtyEvent(event: PtyEvent): void {
    if (event.type === 'started') {
      this.status = 'running'
      this.onStatusChange?.('running', null)
    } else if (event.type === 'exit') {
      const code = event.code ?? null
      this.status = 'exited'
      this.onStatusChange?.('exited', code)
    } else if (event.type === 'error') {
      this.term.writeln(`\r\n\x1b[31m${event.message}\x1b[0m`)
      this.status = 'failed'
      this.onStatusChange?.('failed', null)
    }
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
    this.detach()
    for (const d of this.disposers) d.dispose()
    this.disposers.length = 0
    // Kill the PTY too; a lingering task shouldn't outlive its tab.
    if (this.status === 'starting' || this.status === 'running') {
      backend.tasks.stop(this.runId).catch(() => {})
    }
    this.term.dispose()
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
