import { Channel, invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { LspEvent, SessionStartInfo } from '$lib/types/backend'
import type {
  HostTransport,
  LspStreamRequest,
  PtySinks,
  SessionStreamRequest,
  StreamHandle,
  TaskStreamRequest
} from './transport'

/** Channels exist before invoke so startup output cannot arrive ahead of its listeners. */
function openChannels<T>(
  command: string,
  params: object,
  handlers: Record<string, (message: never) => void>
): StreamHandle<T> {
  let disposed = false
  const channels: Record<string, Channel<never>> = {}
  for (const [name, handler] of Object.entries(handlers)) {
    const channel = new Channel<never>()
    channel.onmessage = (message) => {
      if (!disposed) handler(message)
    }
    channels[name] = channel
  }
  return {
    ready: invoke<T>(command, { ...params, ...channels }),
    dispose() {
      if (disposed) return
      disposed = true
      for (const channel of Object.values(channels)) channel.onmessage = () => {}
    }
  }
}

export function openDesktopPtyStream<T>(command: string, params: object, sinks: PtySinks): StreamHandle<T> {
  return openChannels<T>(command, params, {
    output: (bytes: number[]) => sinks.onOutput(new Uint8Array(bytes)),
    events: sinks.onEvent
  })
}

function openStream(request: SessionStreamRequest, sinks: PtySinks): StreamHandle<SessionStartInfo>
function openStream(request: TaskStreamRequest, sinks: PtySinks): StreamHandle<void>
function openStream(request: LspStreamRequest, sinks: { onEvent: (event: LspEvent) => void }): StreamHandle<void>
function openStream(
  request: SessionStreamRequest | TaskStreamRequest | LspStreamRequest,
  sinks: PtySinks | { onEvent: (event: LspEvent) => void }
): StreamHandle<SessionStartInfo | void> {
  return request.method === 'lsp_start'
    ? openChannels(request.method, request.params, { events: sinks.onEvent })
    : openDesktopPtyStream(request.method, request.params, sinks as PtySinks)
}

export const desktopHostTransport: HostTransport = {
  call<T>(method: string, params: Record<string, unknown>): Promise<T> {
    return invoke<T>(method, params)
  },
  subscribe<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    return listen<T>(event, ({ payload }) => handler(payload))
  },
  openStream
}
