// Browser control wire conversion. Nested domain values belong to their own schemas.
export const MAX_FRAME_BYTES = 64 * 1024 * 1024
export const DISCONNECTED = 'Disconnected from server'
export const OUTCOME_UNKNOWN = 'Disconnected from server; operation outcome may be unknown'

export function utf8ByteLength(value: string): number {
  let bytes = value.length
  for (let index = 0; index < value.length; index++) {
    const code = value.charCodeAt(index)
    if (code < 0x80) continue
    if (code < 0x800) {
      bytes++
    } else if (
      code >= 0xd800 &&
      code <= 0xdbff &&
      value.charCodeAt(index + 1) >= 0xdc00 &&
      value.charCodeAt(index + 1) <= 0xdfff
    ) {
      bytes += 2
      index++
    } else {
      bytes += 2
    }
  }
  return bytes
}

const globalMethods: Record<string, true> = {
  settings_get: true,
  settings_set_nix: true,
  settings_set_formatting: true,
  settings_set_provider_config: true,
  lsp_set_server_config: true,
  folder_home: true,
  folder_working_directory: true
}

export function toWireParams(method: string, params: object): Record<string, unknown> {
  const result: Record<string, unknown> = {}
  for (const [key, value] of Object.entries(params)) {
    if (key === 'folderPath' && globalMethods[method]) continue
    result[key.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`)] = value
  }
  return result
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function field(value: Record<string, unknown>, key: string): string {
  const result = value[key]
  if (typeof result !== 'string') throw new Error(`Invalid wire error: ${key}`)
  return result
}

/** Preserve ApiError's structured variants for frontend conflict/limit decisions. */
export function normalizeWireError(value: unknown): Error | Record<string, unknown> {
  if (!record(value) || typeof value.kind !== 'string') return new Error('Remote error: malformed wire error')
  switch (value.kind) {
    case 'branch_unmerged':
      return { kind: 'branchUnmerged', branch: field(value, 'branch'), message: field(value, 'message') }
    case 'dirty_worktree':
      return { kind: 'dirtyWorktree', message: field(value, 'message') }
    case 'conflict':
      return { kind: 'conflict', currentVersion: field(value, 'current_version') }
    case 'deleted':
      return { kind: 'deleted', path: field(value, 'path') }
    case 'lsp_already_active':
      return { kind: 'lspAlreadyActive', sessionId: field(value, 'session_id') }
    case 'too_large': {
      const { size, limit } = value
      if (!Number.isSafeInteger(size) || !Number.isSafeInteger(limit) || Number(size) < 0 || Number(limit) < 0) {
        throw new Error('Invalid wire error: size or limit')
      }
      return { kind: 'tooLarge', size, limit, message: `File is ${size} bytes; the read limit is ${limit} bytes` }
    }
    default: {
      const prefixes: Record<string, string> = {
        database: 'Database error',
        pty: 'PTY error',
        io: 'IO error',
        not_found: 'Not found',
        invalid_argument: 'Invalid argument',
        internal: 'Internal error',
        unauthorized: 'Remote error'
      }
      return new Error(`${prefixes[value.kind] ?? 'Remote error'}: ${field(value, 'message')}`)
    }
  }
}

export type ControlMessage =
  | { ready: { connection_id: string; controller_token: string } }
  | { busy: true }
  | { revoked: true }
  | { closed: true }
  | { error: Record<string, unknown> & { kind: string } }
  | { ping: number }
  | { id: number; response: { Ok: { method: string; params: unknown } } | { Err: unknown } }
  | { event: { kind: 'workbenches_changed'; payload: null } | { kind: string; payload: unknown } }

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export function decodeControlMessage(raw: string): ControlMessage {
  if (raw.length > MAX_FRAME_BYTES || (raw.length * 3 > MAX_FRAME_BYTES && utf8ByteLength(raw) > MAX_FRAME_BYTES)) {
    throw new Error('Control message exceeds frame limit')
  }
  let value: unknown
  try {
    value = JSON.parse(raw)
  } catch {
    throw new Error('Invalid control message JSON')
  }
  if (!record(value)) throw new Error('Invalid control message')
  if ('ping' in value) {
    if (Object.keys(value).length !== 1 || !Number.isSafeInteger(value.ping) || Number(value.ping) < 0)
      throw new Error('Invalid control heartbeat')
    return value as ControlMessage
  }
  if ('ready' in value) {
    const ready = value.ready
    if (
      Object.keys(value).length !== 1 ||
      !record(ready) ||
      Object.keys(ready).length !== 2 ||
      typeof ready.connection_id !== 'string' ||
      !UUID.test(ready.connection_id) ||
      typeof ready.controller_token !== 'string' ||
      !UUID.test(ready.controller_token)
    ) {
      throw new Error('Invalid control ready message')
    }
    return value as ControlMessage
  }
  for (const terminal of ['busy', 'revoked', 'closed'] as const) {
    if (terminal in value) {
      if (Object.keys(value).length !== 1 || value[terminal] !== true)
        throw new Error(`Invalid control ${terminal} message`)
      return value as ControlMessage
    }
  }
  if ('error' in value) {
    if (Object.keys(value).length !== 1 || !record(value.error) || typeof value.error.kind !== 'string')
      throw new Error('Invalid control error message')
    return value as ControlMessage
  }
  if ('id' in value) {
    if (
      Object.keys(value).length !== 2 ||
      !Number.isSafeInteger(value.id) ||
      Number(value.id) < 0 ||
      !record(value.response) ||
      Object.keys(value.response).length !== 1
    ) {
      throw new Error('Invalid control response ID or envelope')
    }
    const response = value.response
    if ('Ok' in response) {
      if (!record(response.Ok) || typeof response.Ok.method !== 'string' || !('params' in response.Ok)) {
        throw new Error('Invalid control success response')
      }
    } else if (!('Err' in response) || Object.keys(response).length !== 1) {
      throw new Error('Invalid control error response')
    }
    return value as ControlMessage
  }
  if (
    Object.keys(value).length === 1 &&
    'event' in value &&
    record(value.event) &&
    typeof value.event.kind === 'string' &&
    'payload' in value.event
  ) {
    return value as ControlMessage
  }
  throw new Error('Invalid control message')
}
