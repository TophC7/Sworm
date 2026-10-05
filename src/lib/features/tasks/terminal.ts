// Minimal xterm wrapper for task PTY runs.
//
// Just mount xterm, pipe output, forward input — no resume tokens, no
// provider-specific behavior.

import { backend } from '$lib/api/backend'
import type { StreamHandle } from '$lib/api/transport'
import { RenderBarrier } from '$lib/features/sessions/terminal/renderBarrier'
import { TERMINAL_OPTIONS, writeAndWait } from '$lib/features/sessions/terminal/xterm'
import { setTaskTabStatus } from '$lib/features/workbench/state.svelte'
import { getErrorMessage } from '$lib/utils/client-error'
import { requireNative } from '$lib/platform'
import type { PtyEvent, TerminalTransferState } from '$lib/types/backend'
import type { TaskRunStatus, TaskTab } from '$lib/features/workbench/model'
import { splitRemotePath } from '$lib/utils/paths'
import { FitAddon } from '@xterm/addon-fit'
import { SerializeAddon } from '@xterm/addon-serialize'
import { WebLinksAddon } from '@xterm/addon-web-links'
import { Terminal, type IDisposable } from '@xterm/xterm'

const textEncoder = new TextEncoder()

export class TaskTerminal {
  private readonly term: Terminal
  private readonly fit: FitAddon
  private readonly serializeAddon: SerializeAddon
  private readonly disposers: IDisposable[] = []
  private readonly hostEl: HTMLDivElement
  private resizeObserver: ResizeObserver | null = null
  private container: HTMLElement | null = null
  private runId: string
  private streamRunId: string | null = null
  private readonly folderPath: string
  private readonly taskId: string
  private readonly attachOnly: boolean
  private readonly activeFilePath: string | null
  private readonly tabId: string
  private disposed = false
  private startPromise: Promise<void> | null = null
  private stopPromise: Promise<void> | null = null
  private released = false
  private spawned = false
  private status: TaskRunStatus | 'idle' = 'idle'
  private stream: StreamHandle<void | number> | null = null
  private readonly barrier = new RenderBarrier()

  constructor(tab: TaskTab, clearBeforeStart = false) {
    this.tabId = tab.id
    this.runId = tab.runId
    this.folderPath = tab.folderPath
    this.attachOnly = tab.attachOnly
    this.taskId = tab.taskId
    this.activeFilePath = tab.activeFilePath

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

    if (clearBeforeStart) this.term.clear()

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

  /** Start or reattach the PTY once. Stop waits for this request to settle. */
  start(): Promise<void> {
    if (this.spawned || this.disposed) return this.startPromise ?? Promise.resolve()
    this.spawned = true
    this.status = 'starting'
    this.barrier.reset()
    const runId = this.runId
    this.streamRunId = runId
    const { cols, rows } = this.term
    let reportedError = false
    const stream = backend.tasks.start(
      runId,
      this.folderPath,
      this.taskId,
      this.activeFilePath,
      cols,
      rows,
      this.attachOnly,
      {
        onOutput: (data) => this.handleOutput(runId, data),
        onEvent: (event) => {
          if (event.run_id === runId && event.type === 'error') reportedError = true
          this.handlePtyEvent(runId, event)
        }
      }
    )
    this.stream = stream
    this.startPromise = (async () => {
      try {
        await stream.ready
      } catch (error) {
        if (!reportedError) {
          this.handlePtyEvent(runId, {
            type: 'error',
            run_id: runId,
            message: getErrorMessage(error)
          })
        }
        if (this.stream === stream) this.releaseStream()
        throw error
      }
    })()
    return this.startPromise
  }

  private handleOutput(runId: string, data: Uint8Array): void {
    if (this.disposed || this.streamRunId !== runId) return
    const sequence = this.barrier.next()
    this.term.write(data, () => this.barrier.markRendered(sequence))
  }

  private handlePtyEvent(runId: string, event: PtyEvent): void {
    if (this.disposed || this.streamRunId !== runId || event.run_id !== runId) return
    if (event.type === 'synced') {
      if (event.sequence !== undefined) this.barrier.seed(event.sequence)
      return
    }
    const sequence = this.barrier.next()
    if (event.type === 'started') {
      this.status = 'running'
      setTaskTabStatus(this.tabId, 'running', null)
      this.barrier.markRendered(sequence)
    } else if (event.type === 'exit') {
      const code = event.code ?? null
      this.status = 'exited'
      setTaskTabStatus(this.tabId, 'exited', code)
      // Keep both ids through Exit: retained replay can still deliver tail
      // output and Synced after the completion event.
      this.barrier.markRendered(sequence)
    } else if (event.type === 'error') {
      this.term.write(textEncoder.encode(`\r\n\x1b[31m${event.message}\x1b[0m\r\n`), () =>
        this.barrier.markRendered(sequence)
      )
      this.status = 'failed'
      setTaskTabStatus(this.tabId, 'failed', null)
    } else {
      this.barrier.markRendered(sequence)
    }
  }

  async exportTransferState(): Promise<TerminalTransferState> {
    const transfers = requireNative().transfers
    const inert = this.status === 'exited' || this.status === 'failed'
    const runId = inert && splitRemotePath(this.folderPath) ? null : this.streamRunId
    const targetSequence = runId ? await transfers.pause(runId) : 0
    if (runId) await this.barrier.waitFor(targetSequence)
    await writeAndWait(this.term, '')
    const buffer = this.term.buffer.active
    return {
      runId,
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
    const transfers = requireNative().transfers
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
    await writeAndWait(this.term, state.serializedBuffer)
    this.term.scrollToLine(state.viewportPosition)
    this.spawned = true
    if (state.runId == null) return
    this.runId = state.runId
    const runId = state.runId
    this.streamRunId = runId

    const stream = transfers.attach(runId, transferId, {
      onOutput: (data) => this.handleOutput(runId, data),
      onEvent: (event) => this.handlePtyEvent(runId, event)
    })
    this.stream = stream
    this.startPromise = stream.ready.then(
      () => {},
      () => {}
    )
    try {
      const seq = await stream.ready
      if (!this.disposed && this.stream === stream) this.barrier.seed(seq)
    } catch (error) {
      if (this.stream === stream) this.releaseStream()
      throw error
    }
  }

  markPtyLost(): void {
    this.status = 'failed'
    setTaskTabStatus(this.tabId, 'failed', null)
  }

  focus(): void {
    this.term.focus()
  }

  /** Stop the PTY without tearing down xterm — allows the user to
   *  keep reading output after the process exits. */
  stopProcess(): Promise<void> {
    if (this.released) return Promise.resolve()
    if (this.stopPromise) return this.stopPromise
    this.stopPromise = (async () => {
      // Even a rejected start may have registered the run before failing.
      await this.startPromise?.catch(() => {})
      await backend.tasks.stop(this.runId)
      this.released = true
      this.releaseStream()
      if (!this.disposed) {
        this.status = 'exited'
        setTaskTabStatus(this.tabId, 'exited', null)
      }
    })().finally(() => {
      this.stopPromise = null
    })
    return this.stopPromise
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    if (this.spawned) void this.stopProcess().catch(() => {})
    this.releaseStream()
    this.disposeSurface()
  }

  release(): void {
    if (this.disposed) return
    this.disposed = true
    this.releaseStream()
    this.disposeSurface()
  }

  private releaseStream(): void {
    this.stream?.dispose()
    this.stream = null
    this.streamRunId = null
  }

  private disposeSurface(): void {
    this.detach()
    for (const disposer of this.disposers) disposer.dispose()
    this.disposers.length = 0
    this.barrier.reset()
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
