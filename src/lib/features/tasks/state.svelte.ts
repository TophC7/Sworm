// Reactive store of parsed task definitions per project.
//
// Fetches lazily on first access and refreshes when the backend emits
// a `tasks-changed` event (triggered by the notify watcher on
// `.sworm/tasks.json`). Never throws on load failure — returns an
// empty list so the palette and menus stay responsive when the file
// is missing or malformed.

import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { backend } from '$lib/api/backend'
import { notify } from '$lib/features/notifications/state.svelte'
import type { TaskDefinition } from '$lib/types/backend'

const TASKS_CHANGED_EVENT = 'tasks-changed'

let tasksByProject = $state<Map<string, TaskDefinition[]>>(new Map())
const loadedProjects = new Set<string>()

let unlisten: UnlistenFn | null = null
let listenerBooted = false

async function ensureListener(): Promise<void> {
  if (listenerBooted) return
  listenerBooted = true
  try {
    unlisten = await listen<string>(TASKS_CHANGED_EVENT, (event) => {
      const projectId = event.payload
      if (loadedProjects.has(projectId)) {
        void refreshTasks(projectId)
      }
    })
  } catch (err) {
    listenerBooted = false
    console.warn('Failed to subscribe to tasks-changed:', err)
  }
}

async function fetchTasks(projectId: string): Promise<TaskDefinition[]> {
  try {
    return await backend.tasks.list(projectId)
  } catch (err) {
    notify.error('Could not load tasks', String(err))
    return []
  }
}

function setProjectTasks(projectId: string, list: TaskDefinition[]): void {
  tasksByProject = new Map(tasksByProject).set(projectId, list)
}

export async function loadTasks(projectId: string): Promise<TaskDefinition[]> {
  void ensureListener()
  const list = await fetchTasks(projectId)
  setProjectTasks(projectId, list)
  loadedProjects.add(projectId)
  return list
}

export async function refreshTasks(projectId: string): Promise<TaskDefinition[]> {
  const list = await fetchTasks(projectId)
  setProjectTasks(projectId, list)
  return list
}

/** Snapshot of the currently cached task list for a project. Empty
 * when `loadTasks` has never been called (caller is responsible for
 * priming the store). */
export function getTasks(projectId: string): TaskDefinition[] {
  return tasksByProject.get(projectId) ?? []
}

/**
 * Reactive read: returns the cached list, kicking off a background
 * fetch on first access for this project. Safe to call from `$derived`
 * contexts — the underlying Map is `$state`, so updates re-run
 * dependent derivations.
 */
export function getTasksReactive(projectId: string): TaskDefinition[] {
  if (!loadedProjects.has(projectId)) {
    void loadTasks(projectId)
  }
  return tasksByProject.get(projectId) ?? []
}

export function findTask(projectId: string, taskId: string): TaskDefinition | null {
  return tasksByProject.get(projectId)?.find((t) => t.id === taskId) ?? null
}

/** For test/cleanup paths; the listener persists across calls. */
export function disposeTasksStore(): void {
  unlisten?.()
  unlisten = null
  listenerBooted = false
  tasksByProject = new Map()
  loadedProjects.clear()
}
