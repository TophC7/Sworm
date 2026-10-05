import { TaskTerminal } from '$lib/features/tasks/terminal'
import { createTerminalRegistry } from '$lib/features/sessions/terminal/terminalRegistry'
import type { TaskTab } from '$lib/features/workbench/model'
import type { TerminalTransferState } from '$lib/types/backend'

type TaskInit = { tab: TaskTab; clearBeforeStart: boolean }

const registry = createTerminalRegistry<TaskInit, TaskTerminal>(
  'task terminal',
  (init) => init.tab.runId,
  (init) => new TaskTerminal(init.tab, init.clearBeforeStart)
)

export const { get, detach, release, dispose, releaseAll, exportTransferState } = registry

export function getOrCreate(tab: TaskTab, clearBeforeStart = false): TaskTerminal {
  return registry.getOrCreate({ tab, clearBeforeStart })
}

export function importTransferState(
  tab: TaskTab,
  state: TerminalTransferState,
  transferId: string
): Promise<void> {
  return registry.importTransferState({ tab, clearBeforeStart: false }, state, transferId)
}
