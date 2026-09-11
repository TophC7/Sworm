import { TaskTerminal, type TaskTerminalInit } from '$lib/features/tasks/terminal'
import type { TerminalTransferState } from '$lib/types/backend'

const runs = new Map<string, TaskTerminal>()

export function getOrCreate(init: TaskTerminalInit): TaskTerminal {
  let terminal = runs.get(init.runId)
  if (!terminal) {
    terminal = new TaskTerminal(init)
    runs.set(init.runId, terminal)
  }
  return terminal
}

export function attach(init: TaskTerminalInit, container: HTMLElement): TaskTerminal {
  const terminal = getOrCreate(init)
  terminal.attach(container)
  return terminal
}

export function detach(runId: string): void {
  runs.get(runId)?.detach()
}

export function detachForTransfer(runId: string): void {
  const terminal = runs.get(runId)
  if (!terminal) return
  terminal.detachForTransfer()
  runs.delete(runId)
}

export async function exportTransferState(runId: string): Promise<TerminalTransferState> {
  const terminal = runs.get(runId)
  if (!terminal) throw new Error(`Unknown task terminal ${runId}`)
  return terminal.exportTransferState()
}

export async function importTransferState(
  init: TaskTerminalInit,
  state: TerminalTransferState,
  transferId: string
): Promise<TaskTerminal> {
  const terminal = getOrCreate(init)
  try {
    await terminal.importTransferState(state, transferId)
    return terminal
  } catch (error) {
    terminal.detachForTransfer()
    runs.delete(init.runId)
    throw error
  }
}

export function dispose(runId: string): void {
  const terminal = runs.get(runId)
  if (!terminal) return
  terminal.dispose()
  runs.delete(runId)
}

export function disposeAll(): void {
  for (const terminal of runs.values()) {
    terminal.detachForWindowTeardown()
  }
  runs.clear()
}

export function get(runId: string): TaskTerminal | undefined {
  return runs.get(runId)
}

export function focus(runId: string): void {
  runs.get(runId)?.focus()
}
