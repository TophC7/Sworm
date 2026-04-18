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
