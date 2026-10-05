import { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'
import { createTerminalRegistry } from '$lib/features/sessions/terminal/terminalRegistry'
import type { TabId } from '$lib/features/workbench/model'

export const {
  getOrCreate,
  get,
  detach,
  focus,
  release,
  dispose,
  releaseAll,
  exportTransferState,
  importTransferState
} = createTerminalRegistry<TabId, TerminalSessionManager>(
  'terminal session',
  (tabId) => tabId,
  (tabId) => new TerminalSessionManager(tabId)
)
