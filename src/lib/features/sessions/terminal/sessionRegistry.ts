import { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'
import type { TabId } from '$lib/features/workbench/model'
import { isTabTransferring } from '$lib/features/workbench/state.svelte'
import type { TerminalTransferState } from '$lib/types/backend'

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

export function detachForTransfer(tabId: TabId): void {
  const manager = sessions.get(tabId)
  if (!manager) return
  manager.detachForTransfer()
  sessions.delete(tabId)
}

export async function exportTransferState(tabId: TabId): Promise<TerminalTransferState> {
  const manager = sessions.get(tabId)
  if (!manager) throw new Error(`Unknown terminal session ${tabId}`)
  return manager.exportTransferState()
}

export async function importTransferState(
  tabId: TabId,
  state: TerminalTransferState,
  transferId: string
): Promise<void> {
  const manager = getOrCreate(tabId)
  try {
    await manager.importTransferState(state, transferId)
  } catch (error) {
    manager.detachForTransfer()
    sessions.delete(tabId)
    throw error
  }
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
    if (isTabTransferring(tabId)) manager.detachForTransfer()
    else manager.dispose()
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
