export function getErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  // Structured backend errors arrive as plain objects; file races have no message field.
  const structured = error as { message?: unknown; kind?: unknown } | null
  if (typeof structured?.message === 'string') return structured.message
  if (structured?.kind === 'conflict') return 'File changed on disk'
  if (structured?.kind === 'deleted') return 'File deleted on disk'
  return String(error)
}

export function describeClientError(error: unknown): string {
  if (error instanceof Error) {
    return error.stack ? `${error.name}: ${error.message}\n${error.stack}` : `${error.name}: ${error.message}`
  }

  if (typeof error === 'string') {
    return error
  }

  try {
    return JSON.stringify(error, null, 2)
  } catch {
    return String(error)
  }
}

export function logClientError(label: string, detail: Record<string, unknown>): void {
  console.error(`[sworm] ${label}`, detail)
  if (typeof window !== 'undefined') {
    ;(window as Window & { __SWORM_LAST_CLIENT_ERROR__?: Record<string, unknown> }).__SWORM_LAST_CLIENT_ERROR__ = detail
  }
}
