import { TaskTerminal, type TaskTerminalInit } from '$lib/features/tasks/terminal'

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

export function dispose(runId: string): void {
  const terminal = runs.get(runId)
  if (!terminal) return
  terminal.dispose()
  runs.delete(runId)
}

export function get(runId: string): TaskTerminal | undefined {
  return runs.get(runId)
}

export function focus(runId: string): void {
  runs.get(runId)?.focus()
}
