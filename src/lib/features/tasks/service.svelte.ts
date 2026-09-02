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
  getActiveTab,
  resetTaskTabForRestart
} from '$lib/features/workbench/state.svelte'
import { findTask } from '$lib/features/tasks/state.svelte'
import * as taskRegistry from '$lib/features/tasks/taskRegistry'

// Tracks the most recently launched task per folder so "Re-run Last
// Task" in the palette can fire without re-prompting the user to pick.
const lastTaskByFolder = new Map<string, string>()

export function rememberLastTask(folderPath: string, taskId: string): void {
  lastTaskByFolder.set(folderPath, taskId)
}

export function getLastTaskId(folderPath: string): string | null {
  return lastTaskByFolder.get(folderPath) ?? null
}

function newRunId(): string {
  return crypto.randomUUID()
}

/** Path of the active text tab, only when it belongs to `folderPath`
 *  so a task never receives a file from another folder. */
function activeFilePathFor(folderPath: string): string | null {
  const active = getActiveTab()
  if (active?.kind !== 'text' || active.folderPath !== folderPath) return null
  return active.filePath
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
 * Open (or activate) a task tab for the given definition.
 *
 * Behavior:
 * - `confirm: true` → prompt before running. Cancel leaves state unchanged.
 * - `singleton: true` + existing live tab → rebind that tab to a
 *   fresh runId, reset its status, and activate it. Honors `clearOnRerun`.
 * - Otherwise → spawn a new task tab.
 */
export async function openTaskTab(
  folderPath: string,
  task: TaskDefinition,
  options: { activeFilePath?: string | null } = {}
): Promise<TabId | null> {
  if (!(await confirmIfRequired(task))) return null

  const icon = normalizeIcon(task)
  const group = normalizeGroup(task)

  if (task.singleton) {
    const existing = findTaskTabByTaskId(folderPath, task.id)
    if (existing) {
      const activeFilePath = activeFilePathFor(folderPath) ?? options.activeFilePath ?? existing.activeFilePath
      const nextRunId = newRunId()
      taskRegistry.dispose(existing.runId)
      resetTaskTabForRestart(existing.id, nextRunId, {
        activeFilePath,
        label: task.label,
        icon,
        group
      })
      rememberLastTask(folderPath, task.id)
      return existing.id
    }
  }

  const activeFilePath = activeFilePathFor(folderPath) ?? options.activeFilePath ?? null
  const runId = newRunId()
  const tabId = addTaskTab(folderPath, {
    runId,
    taskId: task.id,
    activeFilePath,
    label: task.label,
    icon,
    group
  })
  rememberLastTask(folderPath, task.id)
  return tabId
}

/**
 * Launch the most recently run task in this folder. Returns null
 * when no prior task has been launched or the stored task id is no
 * longer present in `.sworm/tasks.json`.
 */
export async function rerunLastTask(folderPath: string): Promise<TabId | null> {
  const taskId = getLastTaskId(folderPath)
  if (!taskId) return null
  return openTaskTabById(folderPath, taskId)
}

/**
 * Look up a task by id in the cache and open it. Returns null when
 * the task is no longer defined — callers should refresh the task
 * list first if they want a stable read.
 */
export async function openTaskTabById(folderPath: string, taskId: string): Promise<TabId | null> {
  const task = findTask(folderPath, taskId)
  if (!task) return null
  return openTaskTab(folderPath, task)
}
