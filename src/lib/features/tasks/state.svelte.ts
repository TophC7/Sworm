// Reactive store of parsed task definitions per folder.
//
// Fetches lazily on first access and refreshes when the backend emits
// a `tasks-changed` event (triggered by the notify watcher on
// `.sworm/tasks.json`). Never throws on load failure — returns an
// empty list so the palette and menus stay responsive when the file
// is missing or malformed.

import { listen } from '@tauri-apps/api/event'
import { backend } from '$lib/api/backend'
import { notify } from '$lib/features/notifications/state.svelte'
import type { TaskDefinition } from '$lib/types/backend'

const TASKS_CHANGED_EVENT = 'tasks-changed'

let tasksByFolder = $state<Map<string, TaskDefinition[]>>(new Map())
const loadedFolders = new Set<string>()

let listenerBooted = false

async function ensureListener(): Promise<void> {
  if (listenerBooted) return
  listenerBooted = true
  try {
    // Payload is the canonical folder path whose tasks.json changed.
    await listen<string>(TASKS_CHANGED_EVENT, (event) => {
      const folderPath = event.payload
      if (loadedFolders.has(folderPath)) {
        void refreshTasks(folderPath)
      }
    })
  } catch (err) {
    listenerBooted = false
    console.warn('Failed to subscribe to tasks-changed:', err)
  }
}

async function fetchTasks(folderPath: string): Promise<TaskDefinition[]> {
  try {
    return await backend.tasks.list(folderPath)
  } catch (err) {
    notify.error('Could not load tasks', String(err))
    return []
  }
}

function setFolderTasks(folderPath: string, list: TaskDefinition[]): void {
  tasksByFolder = new Map(tasksByFolder).set(folderPath, list)
}

export async function loadTasks(folderPath: string): Promise<TaskDefinition[]> {
  void ensureListener()
  const list = await fetchTasks(folderPath)
  setFolderTasks(folderPath, list)
  loadedFolders.add(folderPath)
  return list
}

export async function refreshTasks(folderPath: string): Promise<TaskDefinition[]> {
  const list = await fetchTasks(folderPath)
  setFolderTasks(folderPath, list)
  return list
}

/** Snapshot of the currently cached task list for a folder. Empty
 * when `loadTasks` has never been called (caller is responsible for
 * priming the store). */
export function getTasks(folderPath: string): TaskDefinition[] {
  return tasksByFolder.get(folderPath) ?? []
}

/**
 * Reactive read: returns the cached list, kicking off a background
 * fetch on first access for this folder. Safe to call from `$derived`
 * contexts — the underlying Map is `$state`, so updates re-run
 * dependent derivations.
 */
export function getTasksReactive(folderPath: string): TaskDefinition[] {
  if (!loadedFolders.has(folderPath)) {
    void loadTasks(folderPath)
  }
  return tasksByFolder.get(folderPath) ?? []
}

export function findTask(folderPath: string, taskId: string): TaskDefinition | null {
  return tasksByFolder.get(folderPath)?.find((t) => t.id === taskId) ?? null
}
