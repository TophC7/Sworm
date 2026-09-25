import type { LspEvent, PtyEvent, SessionSpec, SessionStartInfo } from '$lib/types/backend'

export type Unsubscribe = () => void

export interface StreamHandle<T> {
  readonly ready: Promise<T>
  dispose(): void
}

export type PtySinks = {
  onOutput: (bytes: Uint8Array) => void
  onEvent: (event: PtyEvent) => void
}

export type SessionStreamRequest = {
  method: 'session_start'
  params: SessionSpec & { cols: number; rows: number }
}

export type TaskStreamRequest = {
  method: 'tasks_start'
  params: {
    runId: string
    folderPath: string
    taskId: string
    activeFilePath: string | null
    cols: number
    rows: number
    attachOnly: boolean
  }
}

export type LspStreamRequest = {
  method: 'lsp_start'
  params: {
    sessionId: string
    folderPath: string
    serverDefinitionId: string
    rootPath: string
  }
}

export interface HostTransport {
  call<T>(method: string, params: object): Promise<T>
  subscribe<T>(event: string, handler: (payload: T) => void): Promise<Unsubscribe>
  openStream(request: SessionStreamRequest, sinks: PtySinks): StreamHandle<SessionStartInfo>
  openStream(request: TaskStreamRequest, sinks: PtySinks): StreamHandle<void>
  openStream(request: LspStreamRequest, sinks: { onEvent: (event: LspEvent) => void }): StreamHandle<void>
}

let transport: HostTransport | undefined

export function registerHostTransport(value: HostTransport): void {
  transport = value
}

export function getHostTransport(): HostTransport {
  if (!transport) throw new Error('Host transport is not registered')
  return transport
}
