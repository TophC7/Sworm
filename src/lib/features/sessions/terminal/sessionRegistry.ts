import { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'

const sessions = new Map<string, TerminalSessionManager>()

export function getOrCreate(sessionId: string): TerminalSessionManager {
  let manager = sessions.get(sessionId)
  if (!manager) {
    manager = new TerminalSessionManager(sessionId)
    sessions.set(sessionId, manager)
  }
  return manager
}

export async function attach(sessionId: string, container: HTMLElement): Promise<TerminalSessionManager> {
  const manager = getOrCreate(sessionId)
  await manager.attach(container)
  return manager
}

export function detach(sessionId: string): void {
  sessions.get(sessionId)?.detach()
}

export function dispose(sessionId: string): void {
  const manager = sessions.get(sessionId)
  if (!manager) {
    return
  }

  manager.dispose()
  sessions.delete(sessionId)
}

export function disposeAll(): void {
  for (const [sessionId, manager] of sessions) {
    manager.dispose()
    sessions.delete(sessionId)
  }
}

export function get(sessionId: string): TerminalSessionManager | undefined {
  return sessions.get(sessionId)
}

/**
 * Give DOM focus to a specific session's xterm, if we know about it.
 * No-op for unknown ids or managers that haven't mounted yet. Idempotent.
 */
export function focus(sessionId: string): void {
  sessions.get(sessionId)?.focus()
}
