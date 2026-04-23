// Task lifecycle orchestration.
//
// The store (state.svelte.ts) is the cache; this module is the
// behavior layer: opening task tabs, handling singleton semantics,
// showing the confirm prompt, and rebinding singleton tabs on restart.

import { confirmAsync } from '$lib/features/confirm/service.svelte'
import type { TaskDefinition } from '$lib/types/backend'
import type { TabId } from '$lib/features/workbench/model'
import {
  addTaskTab,
  findTaskTabByTaskId,
  focusTab,
  getFocusedTab,
  resetTaskTabForRestart,
  setTaskTabStatus
} from '$lib/features/workbench/state.svelte'
import { findTask } from '$lib/features/tasks/state.svelte'
import * as taskRegistry from '$lib/features/tasks/taskRegistry'

// Tracks the most recently launched task per project so "Re-run Last
// Task" in the palette can fire without re-prompting the user to pick.
const lastTaskByProject = new Map<string, string>()

export function rememberLastTask(projectId: string, taskId: string): void {
  lastTaskByProject.set(projectId, taskId)
}

export function getLastTaskId(projectId: string): string | null {
  return lastTaskByProject.get(projectId) ?? null
}

function newRunId(): string {
  return crypto.randomUUID()
}

function activeFilePathFor(projectId: string): string | null {
  const focused = getFocusedTab(projectId)
  if (focused?.kind !== 'text') return null
  return focused.filePath
}

function normalizeIcon(task: TaskDefinition): string | null {
  return task.icon?.trim() ? task.icon.trim() : null
}

function normalizeGroup(task: TaskDefinition): string | null {
  return task.group?.trim() ? task.group.trim() : null
}

async function confirmIfRequired(task: TaskDefinition): Promise<boolean> {
  if (!task.confirm) return true
  return confirmAsync({
    title: 'Run task',
    message: `Run "${task.label}"?`,
    confirmLabel: 'Run',
    cancelLabel: 'Cancel'
  })
}

/**
 * Open (or focus) a task tab for the given definition.
 *
 * Behavior:
 * - `confirm: true` → prompt before running. Cancel leaves state unchanged.
 * - `singleton: true` + existing live tab → rebind that tab to a
 *   fresh runId, reset its status, and focus it. Honors `clearOnRerun`.
 * - Otherwise → spawn a new task tab.
 */
export async function openTaskTab(
  projectId: string,
  task: TaskDefinition,
  options: { activeFilePath?: string | null } = {}
): Promise<TabId | null> {
  if (!(await confirmIfRequired(task))) return null

  const icon = normalizeIcon(task)
  const group = normalizeGroup(task)

  if (task.singleton) {
    const existing = findTaskTabByTaskId(projectId, task.id)
    if (existing) {
      const activeFilePath = activeFilePathFor(projectId) ?? options.activeFilePath ?? existing.activeFilePath
      const nextRunId = newRunId()
      taskRegistry.dispose(existing.runId)
      resetTaskTabForRestart(projectId, existing.id, nextRunId, {
        activeFilePath,
        label: task.label,
        icon,
        group
      })
      focusTab(projectId, existing.id)
      rememberLastTask(projectId, task.id)
      return existing.id
    }
  }

  const activeFilePath = activeFilePathFor(projectId) ?? options.activeFilePath ?? null
  const runId = newRunId()
  const tabId = addTaskTab(projectId, {
    runId,
    taskId: task.id,
    activeFilePath,
    label: task.label,
    icon,
    group
  })
  rememberLastTask(projectId, task.id)
  return tabId
}

/**
 * Launch the most recently run task in this project. Returns null
 * when no prior task has been launched or the stored task id is no
 * longer present in `.sworm/tasks.json`.
 */
export async function rerunLastTask(projectId: string): Promise<TabId | null> {
  const taskId = getLastTaskId(projectId)
  if (!taskId) return null
  return openTaskTabById(projectId, taskId)
}

/**
 * Look up a task by id in the cache and open it. Returns null when
 * the task is no longer defined — callers should refresh the task
 * list first if they want a stable read.
 */
export async function openTaskTabById(projectId: string, taskId: string): Promise<TabId | null> {
  const task = findTask(projectId, taskId)
  if (!task) return null
  return openTaskTab(projectId, task)
}

/** Propagate a live PTY status change back into the tab model. */
export function reportTaskStatus(
  projectId: string,
  tabId: TabId,
  status: Parameters<typeof setTaskTabStatus>[2],
  exitCode: number | null = null
): void {
  setTaskTabStatus(projectId, tabId, status, exitCode)
}
