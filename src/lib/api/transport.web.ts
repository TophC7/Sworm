import { resolveProjectFile } from '$lib/utils/paths'
import type { AttachMode, FilePasteMapping } from '$lib/types/backend'
import type { HostTransport } from './transport'
import {
  decodeControlMessage,
  DISCONNECTED,
  MAX_FRAME_BYTES,
  normalizeWireError,
  OUTCOME_UNKNOWN,
  toWireParams,
  utf8ByteLength
} from './transport.web.protocol'
import type { FileBytes, FileReadRequest } from './transport.web.files'
import { createWebStreams } from './transport.web.streams'

export type WebConnectionState = 'connecting' | 'connected' | 'reconnecting' | 'busy' | 'revoked' | 'closed' | 'error'

/** Handshake `error` frame: normalized message plus the raw wire kind (`invalid_argument`, ...). */
export interface WebHandshakeError extends Error {
  kind: string
}

export interface WebHostTransportOptions {
  origin: string | URL
  workbenchId: string
  /** Sent on the first connection attempt only; never replayed by reconnect backoff. */
  takeover?: boolean
  /** Terminal states fire once, after every call/stream/waiter has been rejected; `error` carries its cause. */
  onConnectionState(state: WebConnectionState, error?: WebHandshakeError): void
  onReconnected(): void
  onLspDisconnected(serverDefinitionId: string): void
}

export interface WebHostTransport extends HostTransport {
  ready: Promise<void>
  /** Browser-only binary read over the verified FileRead stream; not a host RPC. */
  readFileBytes(request: FileReadRequest): Promise<FileBytes>
  dispose(): void
}

type Pending = {
  method: string
  /** Own `workbench_close`: this page's `closed` frame is its accepted outcome. */
  selfClose: boolean
  resolve(value: unknown): void
  reject(reason: unknown): void
}
type FolderIntent = {
  claimed: boolean
  dirs?: string[]
  revision: number
  watchedRevision: number
  registeredGeneration: number
  queue: Promise<void>
}

const MAX_PENDING = 256
const reconnectDelays = [1000, 2000, 4000, 8000, 16000, 30000]
const eventNames: Record<string, string> = {
  files_changed: 'files-changed',
  git_changed: 'git-changed',
  settings_changed: 'settings-changed',
  recent_folders_changed: 'recent-folders-changed',
  tasks_changed: 'tasks-changed',
  nix_changed: 'nix-changed',
  issues_changed: 'issues-changed',
  workbenches_changed: 'workbenches-changed'
}

export function createWebHostTransport(options: WebHostTransportOptions): WebHostTransport {
  const controlUrl = new URL('/ws', options.origin)
  controlUrl.protocol = controlUrl.protocol === 'https:' ? 'wss:' : 'ws:'
  const streamUrl = new URL('/ws/stream', controlUrl)
  const listeners = new Map<string, Set<(payload: unknown) => void>>()
  const folders = new Map<string, FolderIntent>()
  const pending = new Map<number, Pending>()
  const waiting = new Set<{ resolve(): void; reject(reason: unknown): void }>()
  let nextId = 1
  let generation = 0
  let socket: WebSocket | null = null
  let active: { socket: WebSocket; generation: number; connectionId: string } | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  let livenessTimer: ReturnType<typeof setTimeout> | undefined
  let declarationTimer: ReturnType<typeof setTimeout> | undefined
  let retries = 0
  let disposed = false
  let everReady = false
  // Memory only: a duplicated tab or reload must not inherit automatic-controller authority.
  let controllerToken: string | null = null
  let takeover = options.takeover ?? false
  const lostErrors = new WeakSet<Error>()
  function lostError(message: string): Error {
    const error = new Error(message)
    lostErrors.add(error)
    return error
  }
  const { promise: ready, resolve: readyResolve, reject: readyReject } = Promise.withResolvers<void>()

  function emitLocal(event: string, payload: unknown): void {
    if (disposed) return
    for (const handler of listeners.get(event) ?? []) {
      try {
        handler(payload)
      } catch (error) {
        console.error(`Host event listener failed: ${event}`, error)
      }
    }
  }

  function rawRpc<T>(method: string, params: object, expectedGeneration?: number): Promise<T> {
    const connection = active
    if (
      disposed ||
      !connection ||
      connection.socket.readyState !== WebSocket.OPEN ||
      (expectedGeneration !== undefined && connection.generation !== expectedGeneration)
    ) {
      return Promise.reject(lostError(DISCONNECTED))
    }
    if (!Number.isSafeInteger(nextId)) return Promise.reject(new Error('RPC request ID exhausted'))
    if (pending.size >= MAX_PENDING) return Promise.reject(new Error('Too many pending host requests'))
    const id = nextId++
    let body: string
    try {
      body = JSON.stringify({ id, request: { method, params: toWireParams(method, params) } })
      const byteLength = utf8ByteLength(body)
      if (byteLength > MAX_FRAME_BYTES) throw new Error('RPC request exceeds frame limit')
      if (connection.socket.bufferedAmount + byteLength > MAX_FRAME_BYTES)
        throw new Error('Control send buffer limit exceeded')
    } catch (error) {
      return Promise.reject(error)
    }
    return new Promise<T>((resolve, reject) => {
      pending.set(id, {
        method,
        selfClose: method === 'workbench_close' && 'id' in params && params.id === options.workbenchId,
        resolve: (value) => resolve(value as T),
        reject
      })
      try {
        connection.socket.send(body)
      } catch {
        lose(connection.socket, connection.generation)
      }
    })
  }

  const streams = createWebStreams({
    rpc: rawRpc,
    openSocket(expectedGeneration?: number) {
      const connection = active
      if (
        !connection ||
        disposed ||
        connection.socket.readyState !== WebSocket.OPEN ||
        (expectedGeneration !== undefined && connection.generation !== expectedGeneration)
      )
        return null
      const url = new URL(streamUrl)
      url.searchParams.set('connection_id', connection.connectionId)
      const child = new WebSocket(url)
      child.binaryType = 'arraybuffer'
      return { socket: child, generation: connection.generation }
    },
    onLspDisconnected(id: string) {
      if (!disposed) options.onLspDisconnected(id)
    },
    emitLocal
  })

  function waitForReady(): Promise<void> {
    if (disposed) return Promise.reject(lostError(DISCONNECTED))
    if (active?.socket.readyState === WebSocket.OPEN) return Promise.resolve()
    return new Promise<void>((resolve, reject) => {
      waiting.add({ resolve, reject })
    })
  }

  async function snapshotGet<T>(params: object): Promise<T> {
    while (!disposed) {
      await waitForReady()
      const current = active?.generation
      try {
        return await rawRpc<T>('app_state_get', params, current)
      } catch (error) {
        if (disposed || !(error instanceof Error && lostErrors.has(error))) throw error
      }
    }
    throw new Error(DISCONNECTED)
  }

  function folder(path: string): FolderIntent {
    let intent = folders.get(path)
    if (!intent) {
      intent = { claimed: false, revision: 0, watchedRevision: -1, registeredGeneration: 0, queue: Promise.resolve() }
      folders.set(path, intent)
    }
    return intent
  }

  async function reconcile(
    path: string,
    intent: FolderIntent,
    targetGeneration: number,
    directWatch = false,
    claimOnly = false
  ): Promise<void> {
    while (active?.generation === targetGeneration && !disposed) {
      const revision = intent.revision
      if (!intent.claimed) {
        if (
          directWatch &&
          intent.dirs &&
          (intent.registeredGeneration !== targetGeneration || intent.watchedRevision !== revision)
        ) {
          const dirs = intent.dirs
          await rawRpc('files_watch_dirs', { projectPath: path, dirs }, targetGeneration)
          intent.registeredGeneration = targetGeneration
          if (!intent.claimed && intent.revision === revision && active?.generation === targetGeneration) {
            intent.watchedRevision = revision
            emitLocal('files-changed', { folder_path: path, dirs })
            return
          }
          continue
        }
        if (intent.registeredGeneration === targetGeneration) {
          await rawRpc('folder_release', { folderPath: path }, targetGeneration)
          intent.registeredGeneration = 0
          intent.watchedRevision = -1
          continue
        }
        if (!intent.dirs && folders.get(path) === intent) folders.delete(path)
        return
      }
      if (intent.registeredGeneration !== targetGeneration) {
        await rawRpc('folder_claim', { folderPath: path }, targetGeneration)
        intent.registeredGeneration = targetGeneration
        intent.watchedRevision = -1
        if (!intent.claimed) continue // A release raced the admitted claim; undo it on this socket.
      }
      if (claimOnly) return
      if (intent.dirs && intent.watchedRevision !== intent.revision) {
        const dirs = intent.dirs
        await rawRpc('files_watch_dirs', { projectPath: path, dirs }, targetGeneration)
        if (intent.claimed && intent.revision === revision && active?.generation === targetGeneration) {
          intent.watchedRevision = revision
          emitLocal('files-changed', { folder_path: path, dirs })
        }
        continue
      }
      if (revision === intent.revision) return
    }
    throw new Error(DISCONNECTED)
  }

  function enqueue(
    path: string,
    intent: FolderIntent,
    targetGeneration: number,
    directWatch = false,
    claimOnly = false
  ): Promise<void> {
    const operation = intent.queue
      .catch(() => {})
      .then(() => reconcile(path, intent, targetGeneration, directWatch, claimOnly))
    intent.queue = operation.catch(() => {})
    return operation
  }

  async function restoreDeclarations(currentGeneration: number, reconnected: boolean, attempt = 0): Promise<void> {
    const claims = await Promise.allSettled(
      [...folders]
        .filter(([, intent]) => intent.claimed)
        .map(([path, intent]) => enqueue(path, intent, currentGeneration, false, true))
    )
    if (active?.generation !== currentGeneration || active.socket.readyState !== WebSocket.OPEN || disposed) return
    for (const result of claims) {
      if (result.status === 'rejected') console.error('Failed to restore folder claim', result.reason)
    }
    const watches = await Promise.allSettled(
      [...folders]
        .filter(([, intent]) => intent.claimed && intent.dirs && intent.registeredGeneration === currentGeneration)
        .map(([path, intent]) => enqueue(path, intent, currentGeneration))
    )
    if (active?.generation !== currentGeneration || active.socket.readyState !== WebSocket.OPEN || disposed) return
    for (const result of watches) {
      if (result.status === 'rejected') console.error('Failed to restore file registration', result.reason)
    }
    if ([...claims, ...watches].some((result) => result.status === 'rejected')) {
      declarationTimer = setTimeout(
        () => {
          declarationTimer = undefined
          if (active?.generation === currentGeneration && !disposed)
            void restoreDeclarations(currentGeneration, reconnected, attempt + 1)
        },
        reconnectDelays[Math.min(attempt, reconnectDelays.length - 1)]
      )
      return
    }
    if (reconnected) options.onReconnected()
  }

  function retry(): void {
    if (disposed || timer || socket) return
    const delay = reconnectDelays[Math.min(retries++, reconnectDelays.length - 1)]
    timer = setTimeout(() => {
      timer = undefined
      connect()
    }, delay)
  }

  function lose(current: WebSocket, token: number): void {
    if (socket !== current || disposed) return
    clearTimeout(livenessTimer)
    clearTimeout(declarationTimer)
    livenessTimer = declarationTimer = undefined
    const wasActive = active?.generation === token
    active = null
    if (wasActive) streams.controlLost(token)
    socket = null
    for (const operation of pending.values()) operation.reject(lostError(OUTCOME_UNKNOWN))
    pending.clear()
    current.close()
    if (!disposed) options.onConnectionState('reconnecting')
    retry()
  }

  function expectHeartbeat(ws: WebSocket, token: number): void {
    clearTimeout(livenessTimer)
    livenessTimer = setTimeout(() => lose(ws, token), 30_000)
  }

  function connect(): void {
    if (disposed || socket) return
    const token = ++generation
    const mode: AttachMode = takeover
      ? { kind: 'takeover' }
      : controllerToken
        ? { kind: 'resume', controller_token: controllerToken }
        : { kind: 'open' }
    const hello = JSON.stringify({ hello: { workbench_id: options.workbenchId, mode } })
    takeover = false
    let ws: WebSocket
    try {
      ws = new WebSocket(controlUrl)
    } catch (error) {
      console.error('Control connection failed', error)
      options.onConnectionState('reconnecting')
      retry()
      return
    }
    socket = ws
    expectHeartbeat(ws, token)
    ws.onopen = () => {
      if (socket !== ws || disposed) return
      try {
        ws.send(hello)
      } catch {
        lose(ws, token)
      }
    }
    ws.onmessage = (message) => {
      if (socket !== ws || disposed || token !== generation) return
      if (typeof message.data !== 'string') {
        lose(ws, token)
        return
      }
      try {
        const frame = decodeControlMessage(message.data)
        if ('revoked' in frame) return terminate('revoked', new Error('Another page controls this workbench'))
        if ('closed' in frame) return terminate('closed', new Error('Workbench closed'))
        if ('busy' in frame || 'error' in frame) {
          if (active) throw new Error('Handshake result after ready')
          if ('busy' in frame) return terminate('busy', new Error('This workbench is controlled by another page'))
          const normalized = normalizeWireError(frame.error)
          const error = (
            normalized instanceof Error ? normalized : new Error(`Remote error: ${frame.error.kind}`)
          ) as WebHandshakeError
          error.kind = frame.error.kind
          return terminate('error', error)
        }
        if ('ready' in frame) {
          if (active) throw new Error('Duplicate control ready message')
          // A mismatched lease is refused as revoked, so a new token here means the
          // workbench was pruned or closed while offline and recreated: adopt it.
          controllerToken = frame.ready.controller_token
          const wasReady = everReady
          active = { socket: ws, generation: token, connectionId: frame.ready.connection_id }
          expectHeartbeat(ws, token)
          retries = 0
          options.onConnectionState('connected')
          if (disposed || socket !== ws) return
          if (!everReady) {
            everReady = true
            readyResolve()
          }
          for (const waiter of waiting) waiter.resolve()
          waiting.clear()
          streams.controlReady(token)
          void restoreDeclarations(token, wasReady)
          return
        }
        if (!active || active.generation !== token) throw new Error('Control message before ready')
        if ('ping' in frame) {
          if (ws.bufferedAmount > MAX_FRAME_BYTES - 64) throw new Error('Control send buffer limit exceeded')
          ws.send(JSON.stringify({ pong: frame.ping }))
          expectHeartbeat(ws, token)
          return
        }
        if ('event' in frame) {
          const name = eventNames[frame.event.kind]
          if (!name) throw new Error(`Unknown host event: ${frame.event.kind}`)
          const payload =
            frame.event.kind === 'workbenches_changed'
              ? { server: null }
              : frame.event.kind === 'nix_changed' || frame.event.kind === 'issues_changed'
                ? { folderPath: frame.event.payload }
                : frame.event.payload
          emitLocal(name, payload)
          return
        }
        const operation = pending.get(frame.id)
        if (!operation) throw new Error('Unexpected control reply ID')
        if ('Err' in frame.response) {
          const error = normalizeWireError(frame.response.Err)
          pending.delete(frame.id)
          operation.reject(error)
          return
        }
        pending.delete(frame.id)
        if ('Ok' in frame.response) {
          if (frame.response.Ok.method !== operation.method) {
            operation.reject(new Error(`Unexpected reply method: ${frame.response.Ok.method}`))
            throw new Error('Control reply method mismatch')
          }
          operation.resolve(frame.response.Ok.params)
        } else throw new Error('Invalid control response')
      } catch (error) {
        console.error('Control protocol error', error)
        lose(ws, token)
      }
    }
    ws.onerror = () => lose(ws, token)
    ws.onclose = () => lose(ws, token)
  }

  /**
   * The single inert end state for dispose and terminal frames: nothing can
   * reconnect, recover, or reach the server afterwards. Only a `closed` frame
   * accepts this page's own Close, whose reply retirement may discard.
   */
  function shutdown(callReason: Error, reason: Error, closed = false): boolean {
    if (disposed) return false
    disposed = true
    clearTimeout(timer)
    clearTimeout(livenessTimer)
    clearTimeout(declarationTimer)
    timer = livenessTimer = declarationTimer = undefined
    const old = socket
    socket = null
    active = null
    streams.dispose()
    old?.close()
    for (const operation of pending.values()) {
      if (closed && operation.selfClose) operation.resolve(undefined)
      else operation.reject(callReason)
    }
    pending.clear()
    for (const waiter of waiting) waiter.reject(reason)
    waiting.clear()
    if (!everReady) readyReject(reason)
    folders.clear()
    listeners.clear()
    return true
  }

  function terminate(state: 'busy' | 'revoked' | 'closed' | 'error', reason: Error): void {
    if (shutdown(reason, reason, state === 'closed'))
      options.onConnectionState(state, state === 'error' ? (reason as WebHandshakeError) : undefined)
  }

  function subscribe<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    if (disposed) return Promise.reject(new Error(DISCONNECTED))
    let handlers = listeners.get(event)
    if (!handlers) {
      handlers = new Set()
      listeners.set(event, handlers)
    }
    const listener = handler as (payload: unknown) => void
    handlers.add(listener)
    let removed = false
    return Promise.resolve(() => {
      if (removed) return
      removed = true
      handlers.delete(listener)
      if (!handlers.size) listeners.delete(event)
    })
  }

  async function call<T>(method: string, params: object): Promise<T> {
    if (method === 'app_state_get') return snapshotGet<T>(params)
    const streamCall = streams.call<T>(method, params)
    if (streamCall !== undefined) return streamCall
    if (method === 'folder_claim' || method === 'folder_release' || method === 'files_watch_dirs') {
      const values = params as { folderPath?: string; projectPath?: string; dirs?: string[] }
      const path = method === 'files_watch_dirs' ? values.projectPath : values.folderPath
      if (typeof path !== 'string') throw new Error('Invalid folder path')
      const intent = folder(path)
      intent.revision++
      if (method === 'folder_claim') intent.claimed = true
      if (method === 'folder_release') {
        intent.claimed = false
        intent.dirs = undefined
      }
      if (method === 'files_watch_dirs') intent.dirs = values.dirs?.slice()
      const token = active?.generation
      if (token === undefined) {
        if (!intent.claimed && !intent.dirs && folders.get(path) === intent) folders.delete(path)
        throw new Error(DISCONNECTED)
      }
      await enqueue(path, intent, token, method === 'files_watch_dirs')
      return undefined as T
    }
    const result = await rawRpc<T>(method, params)
    if (method === 'file_rename') {
      const { projectPath, oldPath, newPath } = params as { projectPath: string; oldPath: string; newPath: string }
      emitLocal('file-path-changed', {
        folderPath: projectPath,
        oldPath: resolveProjectFile(projectPath, oldPath),
        newPath: resolveProjectFile(projectPath, newPath)
      })
    } else if (method === 'file_delete') {
      const { projectPath, filePath } = params as { projectPath: string; filePath: string }
      emitLocal('file-deleted', { filePath: resolveProjectFile(projectPath, filePath) })
    } else if (method === 'file_paste' && (params as { op?: string }).op === 'cut') {
      const { projectPath } = params as { projectPath: string }
      for (const mapping of result as FilePasteMapping[]) {
        emitLocal('file-deleted', { filePath: resolveProjectFile(projectPath, mapping.destination) })
        emitLocal('file-path-changed', {
          folderPath: projectPath,
          oldPath: mapping.source,
          newPath: resolveProjectFile(projectPath, mapping.destination)
        })
      }
    }
    return result
  }

  options.onConnectionState('connecting')
  connect()
  return {
    ready,
    call,
    subscribe,
    openStream: streams.openStream,
    readFileBytes: streams.readFileBytes,
    dispose() {
      shutdown(lostError(OUTCOME_UNKNOWN), new Error(DISCONNECTED))
    }
  }
}
