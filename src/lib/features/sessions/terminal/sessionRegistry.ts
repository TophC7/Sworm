import { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'
import type { TabId } from '$lib/features/workbench/model'

const sessions = new Map<TabId, TerminalSessionManager>()

export function getOrCreate(tabId: TabId): TerminalSessionManager {
  let manager = sessions.get(tabId)
  if (!manager) {
    manager = new TerminalSessionManager(tabId)
    sessions.set(tabId, manager)
  }
  return manager
}

export async function attach(tabId: TabId, container: HTMLElement): Promise<TerminalSessionManager> {
  const manager = getOrCreate(tabId)
  await manager.attach(container)
  return manager
}

export function detach(tabId: TabId): void {
  sessions.get(tabId)?.detach()
}

export function dispose(tabId: TabId): void {
  const manager = sessions.get(tabId)
  if (!manager) {
    return
  }

  manager.dispose()
  sessions.delete(tabId)
}

export function disposeAll(): void {
  for (const [tabId, manager] of sessions) {
    manager.dispose()
    sessions.delete(tabId)
  }
}

export function get(tabId: TabId): TerminalSessionManager | undefined {
  return sessions.get(tabId)
}

/**
 * Give DOM focus to a specific session tab's xterm, if we know about it.
 * No-op for unknown ids or managers that haven't mounted yet. Idempotent.
 */
export function focus(tabId: TabId): void {
  sessions.get(tabId)?.focus()
}
