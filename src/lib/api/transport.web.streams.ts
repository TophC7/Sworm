import type { FileContent, LspEvent, PtyEvent, SessionStartInfo } from '$lib/types/backend'
import type {
  HostTransport,
  LspStreamRequest,
  PtySinks,
  SessionStreamRequest,
  StreamHandle,
  TaskStreamRequest
} from './transport'
import { FileAssembly, MAX_FILE_BYTES, type FileBytes, type FileReadRequest } from './transport.web.files'
import { MAX_FRAME_BYTES, normalizeWireError } from './transport.web.protocol'

const MAX_OPEN_BYTES = 64 * 1024
const MAX_FILE_CHUNK_BYTES = 1024 * 1024
const MAX_BUFFERED_BYTES = MAX_FRAME_BYTES
const disconnected = () => new Error('Disconnected from server; operation outcome may be unknown')
const cancelled = () => new Error('Invalid argument: File read cancelled')
const encoder = new TextEncoder()
const decoder = new TextDecoder('utf-8', { fatal: true })
const fileTextDecoder = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true })

type GenerationSocket = { socket: WebSocket; generation: number }
type Open = Record<string, unknown>
type LiveSocket = GenerationSocket & {
  send(body: Uint8Array, tag: 0 | 1): Promise<void>
  close(): void
  closed: Promise<void>
}
type Frame = { tag: 0 | 1; body: Uint8Array }
type Rpc = <T>(method: string, params: object, generation?: number) => Promise<T>

type WebStreamsOptions = {
  rpc: Rpc
  openSocket: (expectedGeneration?: number) => GenerationSocket | null
  onLspDisconnected: (serverDefinitionId: string) => void
  emitLocal: (event: string, payload: unknown) => void
}

function checkedInteger(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) throw new Error(`Invalid ${name}`)
  return value as number
}

/** Decode a full tagged PTY output frame without copying its output bytes. */
function parsePtyRaw(frame: Uint8Array, expectedOffset: number): Uint8Array {
  if (frame[0] !== 1 || frame.length < 9 || frame.length - 1 > MAX_FRAME_BYTES) {
    throw new Error('Invalid PTY output frame')
  }
  const offset = new DataView(frame.buffer, frame.byteOffset + 1, 8).getBigUint64(0, false)
  if (offset > BigInt(Number.MAX_SAFE_INTEGER) || Number(offset) !== checkedInteger(expectedOffset, 'PTY cursor')) {
    throw new Error('Non-contiguous PTY output offset')
  }
  if (frame.length - 9 > Number.MAX_SAFE_INTEGER - expectedOffset) throw new Error('PTY output cursor overflow')
  return frame.subarray(9)
}

function jsonFrame(value: object): Uint8Array {
  const body = encoder.encode(JSON.stringify(value))
  if (body.length > MAX_FRAME_BYTES) throw new Error('Stream frame exceeds protocol limit')
  return body
}

/** Installs binary handlers before sending Open. All frame delivery is synchronous and in arrival order. */
function connect(
  options: WebStreamsOptions,
  open: Open,
  expectedGeneration: number | undefined,
  onFrame: (frame: Frame) => void,
  onClose: (error: Error) => void,
  onCreated?: (socket: LiveSocket) => void
): Promise<LiveSocket> {
  const connection = options.openSocket(expectedGeneration)
  if (!connection) return Promise.reject(disconnected())
  const { socket } = connection
  socket.binaryType = 'arraybuffer'
  const opened = Promise.withResolvers<LiveSocket>()
  const closed = Promise.withResolvers<void>()
  const openingTimeout = setTimeout(() => fail(new Error('Stream open timed out')), 30_000)
  let active = true
  const close = () => {
    if (socket.readyState === WebSocket.CONNECTING || socket.readyState === WebSocket.OPEN) socket.close()
  }
  const fail = (error: Error) => {
    if (!active) return
    active = false
    clearTimeout(openingTimeout)
    opened.reject(error)
    close()
    onClose(error)
  }
  const live: LiveSocket = {
    ...connection,
    close,
    closed: closed.promise,
    send: async (body, tag) => {
      if (!active || socket.readyState !== WebSocket.OPEN) throw disconnected()
      if (body.length > MAX_FRAME_BYTES || socket.bufferedAmount + body.length + 1 > MAX_BUFFERED_BYTES) {
        throw new Error('Stream send exceeds protocol byte budget')
      }
      const frame = new Uint8Array(body.length + 1)
      frame[0] = tag
      frame.set(body, 1)
      socket.send(frame)
    }
  }
  onCreated?.(live)
  socket.onopen = () => {
    clearTimeout(openingTimeout)
    try {
      const body = encoder.encode(JSON.stringify(open))
      if (body.length > MAX_OPEN_BYTES) throw new Error('Stream Open exceeds protocol limit')
      socket.send(body)
      opened.resolve(live)
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)))
    }
  }
  socket.onmessage = ({ data }) => {
    try {
      if (!(data instanceof ArrayBuffer)) throw new Error('Expected binary stream frame')
      const bytes = new Uint8Array(data)
      if (bytes.length < 1 || bytes.length - 1 > MAX_FRAME_BYTES || (bytes[0] !== 0 && bytes[0] !== 1)) {
        throw new Error('Invalid stream frame')
      }
      onFrame({ tag: bytes[0], body: bytes.subarray(1) })
    } catch (error) {
      fail(error instanceof Error ? error : new Error(String(error)))
    }
  }
  socket.onerror = () => fail(new Error('Stream socket failed'))
  socket.onclose = () => {
    closed.resolve()
    fail(new Error('Stream socket closed'))
  }
  return opened.promise
}

type PtyOwner = {
  runId: string
  disposed: boolean
  socket: LiveSocket | null
  generation: number | undefined
  cursor: { output_offset: number; event_sequence: number }
  dimensions: { cols: number; rows: number }
  exited: boolean
  started: boolean
  terminal: boolean
  retry: ReturnType<typeof setTimeout> | undefined
  attempts: number
  sinks: PtySinks
  pendingStartup: PromiseWithResolvers<void> | null
}

type LspOwner = {
  sessionId: string
  definitionId: string
  disposed: boolean
  exited: boolean
  socket: LiveSocket | null
  generation: number | undefined
  submitted: boolean
  startup: Promise<void>
  stopPromise: Promise<void> | null
  readyFrame: PromiseWithResolvers<void>
  sinks: { onEvent: (event: LspEvent) => void }
  rejectStartup: (error: Error) => void
}

type FileOwner = {
  requestId: string
  projectPath: string
  filePath: string
  version: string
  size: number
  socket: LiveSocket | null
  assembly: FileAssembly | null
  cancelled: boolean
  done: boolean
  resolve: (value: FileBytes) => void
  reject: (reason: unknown) => void
  finished: PromiseWithResolvers<void>
}

export interface WebStreams {
  call<T>(method: string, params: object): Promise<T> | undefined
  /** Binary-safe project file read sharing the text facade's queue, slots, and cancellation by request ID. */
  readFileBytes(request: FileReadRequest): Promise<FileBytes>
  openStream: HostTransport['openStream']
  controlLost(generation: number): void
  controlReady(generation: number): void
  dispose(): void
}

export function createWebStreams(options: WebStreamsOptions): WebStreams {
  const ptys = new Map<string, PtyOwner>()
  const lsps = new Map<string, LspOwner>()
  const stoppedLsps = new Map<string, Promise<void>>()
  const files = new Map<string, FileOwner>()
  const queue: FileOwner[] = []
  let slots = 0
  let currentGeneration: number | undefined
  let notifyingLoss = false
  let disposed = false

  function sendPtyDimensions(owner: PtyOwner): void {
    if (!owner.socket) return
    void owner.socket.send(jsonFrame({ kind: 'resize', ...owner.dimensions }), 0).catch(() => owner.socket?.close())
  }

  function attachPty(owner: PtyOwner): Promise<void> {
    let created: LiveSocket | null = null
    // Every retry opens only a PTY attachment, never repeats the start RPC.
    return connect(
      options,
      { kind: 'pty', run_id: owner.runId, cursor: owner.cursor },
      currentGeneration,
      ({ tag, body }) => {
        if (owner.disposed || owner.terminal || owner.socket !== created || owner.generation !== currentGeneration)
          return
        if (tag === 1) {
          const full = new Uint8Array(body.buffer, body.byteOffset - 1, body.byteLength + 1)
          const bytes = parsePtyRaw(full, owner.cursor.output_offset)
          owner.sinks.onOutput(bytes)
          owner.cursor.output_offset += bytes.length
          return
        }
        const frame = JSON.parse(decoder.decode(body)) as {
          kind: string
          sequence?: number
          event?: PtyEvent
          lost_bytes?: number
          error?: unknown
        }
        if (frame.kind === 'closed') {
          owner.terminal = true
          const error = normalizeWireError(frame.error)
          owner.sinks.onEvent({
            type: 'error',
            run_id: owner.runId,
            message: error instanceof Error ? error.message : String(error.message ?? error.kind)
          })
          owner.pendingStartup?.reject(error)
          owner.socket?.close()
          return
        }
        if (frame.kind === 'gap') {
          const lost = checkedInteger(frame.lost_bytes, 'PTY gap')
          if (lost > Number.MAX_SAFE_INTEGER - owner.cursor.output_offset) throw new Error('PTY output cursor overflow')
          owner.cursor.output_offset += lost
          owner.sinks.onOutput(encoder.encode(`\r\n[${lost} bytes of terminal history were lost]\r\n`))
          return
        }
        if (frame.kind !== 'event' || !frame.event) throw new Error('Invalid PTY event')
        const sequence = checkedInteger(frame.sequence, 'PTY event sequence')
        if (frame.event.type === 'synced') {
          owner.attempts = 0
          owner.pendingStartup?.resolve()
          owner.sinks.onEvent(frame.event)
          if (owner.exited) {
            owner.terminal = true
            owner.socket?.close()
          }
        } else if (sequence > owner.cursor.event_sequence) {
          owner.sinks.onEvent(frame.event)
          owner.cursor.event_sequence = sequence
          if (frame.event.type === 'exit') owner.exited = true
        }
      },
      () => {
        if (owner.socket !== created) return
        owner.socket = null
        if (owner.disposed || disposed) owner.pendingStartup?.resolve()
        else if (!owner.terminal) schedulePty(owner)
      },
      (socket) => {
        created = socket
        owner.socket = socket
        owner.generation = socket.generation
      }
    ).then((socket) => {
      if (owner.disposed || owner.terminal || owner.socket !== socket || socket.generation !== currentGeneration) {
        socket.close()
        return
      }
      sendPtyDimensions(owner)
    })
  }

  function schedulePty(owner: PtyOwner): void {
    if (owner.retry || !owner.started || owner.disposed || owner.terminal || currentGeneration === undefined) return
    const seconds = Math.min(2 ** owner.attempts++, 30)
    owner.retry = setTimeout(() => {
      owner.retry = undefined
      if (!owner.disposed && !owner.terminal && currentGeneration !== undefined)
        void attachPty(owner).catch(() => schedulePty(owner))
    }, seconds * 1000)
  }

  function openPty(
    request: SessionStreamRequest | TaskStreamRequest,
    sinks: PtySinks
  ): StreamHandle<SessionStartInfo | void> {
    const owner: PtyOwner = {
      runId: request.params.runId,
      disposed: false,
      socket: null,
      generation: undefined,
      cursor: { output_offset: 0, event_sequence: 0 },
      dimensions: { cols: request.params.cols, rows: request.params.rows },
      exited: false,
      started: false,
      terminal: false,
      retry: undefined,
      attempts: 0,
      sinks,
      pendingStartup: null
    }
    const replaced = ptys.get(owner.runId)
    if (replaced) {
      replaced.disposed = true
      replaced.pendingStartup?.resolve()
      clearTimeout(replaced.retry)
      replaced.socket?.close()
    }
    ptys.set(owner.runId, owner)
    // Start is admitted immediately. Disposal never replaces its original metadata or error.
    const startup = options.rpc<SessionStartInfo | void>(request.method, request.params)
    const ready = startup.then(async (result) => {
      if (!owner.disposed && !disposed) {
        owner.started = true
        const attached = Promise.withResolvers<void>()
        owner.pendingStartup = attached
        void attachPty(owner).catch(() => schedulePty(owner))
        try {
          await attached.promise
        } finally {
          owner.pendingStartup = null
        }
      }
      return result
    })
    return {
      ready,
      dispose() {
        if (owner.disposed) return
        owner.disposed = true
        owner.pendingStartup?.resolve()
        clearTimeout(owner.retry)
        owner.socket?.close()
        if (ptys.get(owner.runId) === owner) ptys.delete(owner.runId)
      }
    }
  }

  function lostLsp(owner: LspOwner, error: Error, closeSocket = true): void {
    if (owner.disposed || owner.exited || disposed) return
    owner.exited = true
    owner.readyFrame.reject(error)
    owner.rejectStartup(error)
    // Recovery must record the definition before an exit listener can dispose the stream.
    try {
      options.onLspDisconnected(owner.definitionId)
    } catch (callbackError) {
      console.error('LSP disconnect listener failed', callbackError)
    }
    if (!owner.disposed && !disposed) {
      try {
        owner.sinks.onEvent({ type: 'exit', session_id: owner.sessionId, code: null })
      } catch (callbackError) {
        console.error('LSP exit listener failed', callbackError)
      }
    }
    if (closeSocket) owner.socket?.close()
  }

  function stopLsp(owner: LspOwner): Promise<void> {
    if (owner.stopPromise) return owner.stopPromise
    owner.disposed = true
    owner.stopPromise = (async () => {
      try {
        if (!owner.submitted) {
          owner.readyFrame.reject(new Error('LSP stream disposed before startup'))
          owner.rejectStartup(new Error('LSP stream disposed before startup'))
          if (notifyingLoss) await Promise.resolve()
          return
        }
        const started = await owner.startup.then(
          () => true,
          () => false
        )
        if (
          started &&
          owner.socket &&
          owner.generation !== undefined &&
          owner.generation === currentGeneration &&
          !owner.exited
        ) {
          await options.rpc<void>('lsp_stop', { sessionId: owner.sessionId }, owner.generation)
        }
      } finally {
        owner.socket?.close()
        await owner.socket?.closed
        if (lsps.get(owner.sessionId) === owner) lsps.delete(owner.sessionId)
      }
    })()
    stoppedLsps.set(owner.sessionId, owner.stopPromise)
    void owner.stopPromise.catch(() => {})
    return owner.stopPromise
  }

  function openLsp(request: LspStreamRequest, sinks: { onEvent: (event: LspEvent) => void }): StreamHandle<void> {
    const { sessionId, serverDefinitionId } = request.params
    if (disposed) throw new Error('Disconnected from server')
    if (lsps.has(sessionId)) throw new Error('LSP session id already in use')
    stoppedLsps.delete(sessionId)
    const frame = Promise.withResolvers<void>()
    void frame.promise.catch(() => {})
    const started = Promise.withResolvers<void>()
    const owner: LspOwner = {
      sessionId,
      definitionId: serverDefinitionId,
      disposed: false,
      exited: false,
      submitted: false,
      socket: null,
      generation: undefined,
      startup: started.promise,
      stopPromise: null,
      readyFrame: frame,
      sinks,
      rejectStartup: started.reject
    }
    // Register ownership before opening: Ready may arrive in the first WS message.
    lsps.set(sessionId, owner)
    const handle: StreamHandle<void> = {
      ready: started.promise,
      dispose() {
        if (!owner.disposed) void stopLsp(owner)
      }
    }
    if (currentGeneration === undefined) {
      // A model can attach while the control connection is down. Keep its original
      // rejected handle, but let recovery rebuild its definition after reconnect.
      lostLsp(owner, disconnected())
      return handle
    }
    void connect(
      options,
      { kind: 'lsp', session_id: sessionId },
      currentGeneration,
      ({ tag, body }) => {
        if (tag !== 0) throw new Error('Invalid LSP stream frame')
        const message = JSON.parse(decoder.decode(body)) as { kind: string; event?: LspEvent }
        if (message.kind === 'ready') {
          frame.resolve()
          return
        }
        if (message.kind !== 'event' || !message.event) throw new Error('Invalid LSP event')
        if (message.event.type === 'exit') {
          owner.exited = true
          if (!owner.disposed) sinks.onEvent(message.event)
          frame.reject(new Error('LSP exited before startup completed'))
          owner.socket?.close()
        } else if (!owner.disposed && !owner.exited) sinks.onEvent(message.event)
      },
      (error) => {
        if (owner.disposed) {
          frame.reject(error)
          started.reject(error)
        } else lostLsp(owner, error)
      },
      (socket) => {
        owner.socket = socket
        owner.generation = socket.generation
      }
    )
      .then(async (socket) => {
        owner.socket = socket
        owner.generation = socket.generation
        await frame.promise
        if (owner.exited || owner.disposed) throw disconnected()
        owner.submitted = true
        await options.rpc<void>(request.method, request.params, socket.generation)
        started.resolve()
        if (owner.disposed) void stopLsp(owner)
      })
      .catch((error) => {
        started.reject(error)
        if (owner.disposed) owner.socket?.close()
        else if (!owner.exited && owner.socket?.socket.readyState === WebSocket.OPEN) {
          // A server refusal is not an unexpected socket loss; relinquish its lease.
          owner.disposed = true
          owner.socket.close()
          if (lsps.get(owner.sessionId) === owner) lsps.delete(owner.sessionId)
        } else if (!owner.exited) lostLsp(owner, error instanceof Error ? error : new Error(String(error)))
      })
    return handle
  }

  function rejectFile(owner: FileOwner, error: unknown): void {
    if (owner.done) return
    owner.done = true
    owner.reject(error)
    owner.finished.resolve()
    owner.socket?.close()
  }

  function pumpFiles(): void {
    while (!disposed && slots < 2 && queue.length) {
      const owner = queue.shift()!
      if (owner.done) continue
      slots++
      void runFile(owner).finally(() => {
        slots--
        if (files.get(owner.requestId) === owner) files.delete(owner.requestId)
        pumpFiles()
      })
    }
  }

  async function runFile(owner: FileOwner): Promise<void> {
    try {
      if (owner.cancelled || owner.done) return
      options.emitLocal('file-read-progress', {
        requestId: owner.requestId,
        folderPath: owner.projectPath,
        filePath: owner.filePath,
        bytes: 0,
        total: owner.size
      })
      const socket = await connect(
        options,
        {
          kind: 'file_read',
          project_path: owner.projectPath,
          file_path: owner.filePath,
          version: owner.version
        },
        currentGeneration,
        ({ tag, body }) => {
          if (owner.cancelled || owner.done) return
          if (owner.assembly?.isComplete) throw new Error('Unexpected file frame after completion')
          if (tag === 1) {
            if (body.length > MAX_FILE_CHUNK_BYTES) throw new Error('File read chunk exceeds protocol limit')
            const assembly = (owner.assembly ??= new FileAssembly(owner.size))
            assembly.append(body)
            options.emitLocal('file-read-progress', {
              requestId: owner.requestId,
              folderPath: owner.projectPath,
              filePath: owner.filePath,
              bytes: assembly.bytesReceived,
              total: owner.size
            })
            return
          }
          const message = JSON.parse(decoder.decode(body)) as { kind: string; version?: string; error?: unknown }
          if (message.kind === 'error') {
            rejectFile(owner, normalizeWireError(message.error))
            return
          }
          if (message.kind !== 'complete' || typeof message.version !== 'string')
            throw new Error('Invalid file read terminal frame')
          if (!owner.assembly && owner.size !== 0) throw new Error('File read completed before receiving bytes')
          const assembly = (owner.assembly ??= new FileAssembly(owner.size))
          // Marks completion synchronously, before normal server Close can arrive during digest.
          owner.finished.resolve()
          void assembly
            .complete(message.version)
            .then(owner.resolve, owner.reject)
            .finally(() => {
              owner.done = true
            })
        },
        (error) => {
          if (!owner.assembly?.isComplete)
            rejectFile(
              owner,
              error.message === 'Stream socket closed' ? new Error('File read closed before completion') : error
            )
          else if (error.message !== 'Stream socket closed') rejectFile(owner, error)
        },
        (socket) => {
          owner.socket = socket
        }
      )
      owner.socket = socket
      if (owner.cancelled || owner.done) socket.close()
      // The socket remains open until the terminal frame, or cancellation. The slot is held through native digest.
      await owner.finished.promise
      await owner.assembly?.settled
    } catch (error) {
      rejectFile(owner, error instanceof Error ? error : new Error(String(error)))
    } finally {
      owner.socket?.close()
    }
  }

  function readFile(params: object): Promise<FileBytes> {
    const { requestId, projectPath, filePath, version, size } = params as Record<string, unknown>
    if (
      typeof requestId !== 'string' ||
      encoder.encode(requestId).length === 0 ||
      encoder.encode(requestId).length > 128
    ) {
      return Promise.reject(new Error('Invalid argument: Invalid file-read request id'))
    }
    if (files.has(requestId)) return Promise.reject(new Error('File-read request id already in use'))
    if (disposed || currentGeneration === undefined) return Promise.reject(new Error('Disconnected from server'))
    if (!Number.isSafeInteger(size) || (size as number) < 0 || (size as number) > MAX_FILE_BYTES) {
      return Promise.reject(new Error('Invalid argument: Invalid approved file size'))
    }
    if (typeof projectPath !== 'string' || typeof filePath !== 'string' || typeof version !== 'string') {
      return Promise.reject(new Error('Invalid argument: Invalid file read arguments'))
    }
    const result = Promise.withResolvers<FileBytes>()
    const owner: FileOwner = {
      requestId,
      projectPath,
      filePath,
      version,
      size: size as number,
      socket: null,
      assembly: null,
      cancelled: false,
      done: false,
      resolve: result.resolve,
      reject: result.reject,
      finished: Promise.withResolvers<void>()
    }
    files.set(requestId, owner)
    queue.push(owner)
    pumpFiles()
    return result.promise
  }

  function call<T>(method: string, params: object): Promise<T> | undefined {
    if (disposed) return Promise.reject(new Error('Disconnected from server'))
    if (method === 'file_read_stream')
      return readFile(params).then(({ bytes, version }): FileContent => ({
        content: fileTextDecoder.decode(bytes),
        version
      })) as unknown as Promise<T>
    if (method === 'file_read_stream_cancel') {
      const { requestId } = params as { requestId: string }
      const owner = files.get(requestId)
      if (owner) {
        if (!owner.cancelled && !owner.done) {
          owner.cancelled = true
          owner.assembly?.cancel()
          rejectFile(owner, cancelled())
          if (!owner.assembly) files.delete(requestId)
        }
      }
      return Promise.resolve(undefined as T)
    }
    if (method === 'lsp_stop') {
      const owner = lsps.get((params as { sessionId: string }).sessionId)
      if (owner) return stopLsp(owner).then(() => undefined as T)
      const recorded = stoppedLsps.get((params as { sessionId: string }).sessionId)
      return recorded ? recorded.then(() => undefined as T) : Promise.reject(disconnected())
    }
    if (method === 'lsp_send') {
      const owner = lsps.get((params as { sessionId: string }).sessionId)
      if (!owner?.socket || owner.disposed || owner.exited || owner.generation !== currentGeneration)
        return Promise.reject(disconnected())
      return owner.socket
        .send(jsonFrame({ kind: 'message', payload_json: (params as { messageJson: string }).messageJson }), 0)
        .then(() => undefined as T)
    }
    const pty = /^(session|tasks)_(write|resize)$/.exec(method)
    if (!pty) return undefined
    const { runId } = params as { runId: string }
    const owner = ptys.get(runId)
    if (!owner || owner.disposed) return Promise.reject(disconnected())
    if (pty[2] === 'resize') {
      const { cols, rows } = params as { cols: number; rows: number }
      owner.dimensions = { cols, rows }
      if (currentGeneration === undefined) return Promise.reject(disconnected())
      if (!owner.socket || owner.socket.socket.readyState !== WebSocket.OPEN) return Promise.resolve(undefined as T)
      if (owner.generation !== currentGeneration) return Promise.reject(disconnected())
      return owner.socket
        .send(jsonFrame({ kind: 'resize', cols: owner.dimensions.cols, rows: owner.dimensions.rows }), 0)
        .then(() => undefined as T)
    }
    if (!owner.socket || owner.generation !== currentGeneration) return Promise.reject(disconnected())
    return owner.socket.send(new Uint8Array((params as { data: number[] }).data), 1).then(() => undefined as T)
  }

  function openStream(request: SessionStreamRequest, sinks: PtySinks): StreamHandle<SessionStartInfo>
  function openStream(request: TaskStreamRequest, sinks: PtySinks): StreamHandle<void>
  function openStream(request: LspStreamRequest, sinks: { onEvent: (event: LspEvent) => void }): StreamHandle<void>
  function openStream(
    request: SessionStreamRequest | TaskStreamRequest | LspStreamRequest,
    sinks: PtySinks | { onEvent: (event: LspEvent) => void }
  ): StreamHandle<SessionStartInfo | void> {
    if (disposed) throw new Error('Disconnected from server')
    if (request.method === 'lsp_start') return openLsp(request, sinks as { onEvent: (event: LspEvent) => void })
    return openPty(request, sinks as PtySinks)
  }

  return {
    call,
    readFileBytes: readFile,
    openStream,
    controlLost(generation) {
      if (currentGeneration === generation) currentGeneration = undefined
      const affected = [...lsps.values()].filter((owner) => owner.generation === generation)
      // Record every affected definition before any child socket is closed.
      notifyingLoss = true
      for (const owner of affected) lostLsp(owner, disconnected(), false)
      notifyingLoss = false
      for (const owner of ptys.values()) {
        if (owner.generation !== generation) continue
        owner.socket?.close()
        owner.socket = null
        if (owner.retry !== undefined) {
          clearTimeout(owner.retry)
          owner.retry = undefined
        }
      }
      for (const owner of affected) owner.socket?.close()
      for (const owner of files.values()) rejectFile(owner, disconnected())
    },
    controlReady(generation) {
      if (disposed) return
      currentGeneration = generation
      for (const owner of ptys.values()) if (!owner.socket && !owner.disposed && !owner.terminal) schedulePty(owner)
    },
    dispose() {
      if (disposed) return
      disposed = true
      currentGeneration = undefined
      for (const owner of ptys.values()) {
        owner.pendingStartup?.resolve()
        owner.disposed = true
        clearTimeout(owner.retry)
        owner.socket?.close()
      }
      for (const owner of lsps.values()) {
        owner.disposed = true
        owner.readyFrame.reject(disconnected())
        owner.socket?.close()
      }
      for (const owner of files.values()) {
        owner.assembly?.cancel()
        rejectFile(owner, cancelled())
      }
      ptys.clear()
      lsps.clear()
      files.clear()
      queue.length = 0
    }
  }
}
