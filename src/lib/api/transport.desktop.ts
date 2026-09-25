import { Channel, invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { LspEvent, PtyEvent, SessionStartInfo } from '$lib/types/backend'
import type {
  HostTransport,
  LspStreamRequest,
  PtySinks,
  SessionStreamRequest,
  StreamHandle,
  TaskStreamRequest
} from './transport'

/** Channels exist before invoke so startup output cannot arrive ahead of its listeners. */
export function openDesktopPtyStream<T>(command: string, params: object, sinks: PtySinks): StreamHandle<T> {
  let disposed = false
  const output = new Channel<number[]>()
  const events = new Channel<PtyEvent>()
  output.onmessage = (bytes) => {
    if (!disposed) sinks.onOutput(new Uint8Array(bytes))
  }
  events.onmessage = (event) => {
    if (!disposed) sinks.onEvent(event)
  }
  const ready = invoke<T>(command, { ...params, output, events })
  return {
    ready,
    dispose() {
      if (disposed) return
      disposed = true
      output.onmessage = () => {}
      events.onmessage = () => {}
    }
  }
}

function openDesktopLspStream(
  request: LspStreamRequest,
  sinks: { onEvent: (event: LspEvent) => void }
): StreamHandle<void> {
  let disposed = false
  const events = new Channel<LspEvent>()
  events.onmessage = (event) => {
    if (!disposed) sinks.onEvent(event)
  }
  const ready = invoke<void>(request.method, { ...request.params, events })
  return {
    ready,
    dispose() {
      if (disposed) return
      disposed = true
      events.onmessage = () => {}
    }
  }
}

function openDesktopStream(request: SessionStreamRequest, sinks: PtySinks): StreamHandle<SessionStartInfo>
function openDesktopStream(request: TaskStreamRequest, sinks: PtySinks): StreamHandle<void>
function openDesktopStream(request: LspStreamRequest, sinks: { onEvent: (event: LspEvent) => void }): StreamHandle<void>
function openDesktopStream(
  request: SessionStreamRequest | TaskStreamRequest | LspStreamRequest,
  sinks: PtySinks | { onEvent: (event: LspEvent) => void }
): StreamHandle<SessionStartInfo | void> {
  if (request.method === 'lsp_start') {
    return openDesktopLspStream(request, sinks as { onEvent: (event: LspEvent) => void })
  }
  return openDesktopPtyStream<SessionStartInfo | void>(request.method, request.params, sinks as PtySinks)
}

export const desktopHostTransport: HostTransport = {
  call<T>(method: string, params: Record<string, unknown>): Promise<T> {
    return invoke<T>(method, params)
  },
  subscribe<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    return listen<T>(event, ({ payload }) => handler(payload))
  },
  openStream: openDesktopStream
}
