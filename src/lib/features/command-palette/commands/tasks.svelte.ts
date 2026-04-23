// Command-palette entries for project tasks.
//
// Two surfaces:
// <> `getTaskAppCommands()` returns the small set of dispatch commands
//    visible in the default palette — "Show Tasks" and "Re-run Last
//    Task". Keeps the main palette uncluttered.
// <> `getTaskPaletteGroups()` returns the actual task list, shown when
//    the search starts with `!` (analogous to `>` for editor commands).

import type { Component } from 'svelte'
import type { Command, CommandGroup } from './types'
import { getTasksReactive } from '$lib/features/tasks/state.svelte'
import { getLastTaskId, openTaskTab, rerunLastTask } from '$lib/features/tasks/service.svelte'
import { openCommandPaletteWithSearch } from '$lib/features/command-palette/state.svelte'
import { getActiveProjectId } from '$lib/features/workbench/state.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { Play, RotateCw } from '$lib/icons/lucideExports'

const TASK_SEARCH_PREFIX = '! '

export function getTaskAppCommands(): CommandGroup[] {
  const projectId = getActiveProjectId()
  if (!projectId) return []

  // Touch the reactive task list so this group refreshes when tasks
  // load or change on disk. The returned value is only used for the
  // "Re-run Last Task" subtitle.
  const tasks = getTasksReactive(projectId)
  const lastId = getLastTaskId(projectId)
  const lastLabel = lastId ? (tasks.find((t) => t.id === lastId)?.label ?? lastId) : null

  const commands: Command[] = [
    {
      id: 'tasks.show',
      label: 'Show Tasks',
      icon: Play as Component,
      keywords: ['tasks', 'task', 'run', '!'],
      shortcut: undefined,
      onSelect: () => openCommandPaletteWithSearch(TASK_SEARCH_PREFIX)
    }
  ]

  if (lastLabel) {
    commands.push({
      id: 'tasks.rerun-last',
      label: 'Re-run Last Task',
      subtitle: lastLabel,
      icon: RotateCw as Component,
      keywords: ['tasks', 'rerun', 'repeat', 'last', lastLabel],
      onSelect: () => {
        void rerunLastTask(projectId).then((tabId) => {
          if (tabId === null) {
            notify.error('Cannot re-run task', 'The last task is no longer defined in .sworm/tasks.json')
          }
        })
      }
    })
  }

  return [{ heading: 'Tasks', commands }]
}

export function getTaskPaletteGroups(): CommandGroup[] {
  const projectId = getActiveProjectId()
  if (!projectId) return []

  const tasks = getTasksReactive(projectId)
  if (tasks.length === 0) return []

  // Group by the `group` field; ungrouped tasks land in a default
  // "Tasks" heading so the palette always shows at least one group.
  const buckets = new Map<string, Command[]>()
  for (const task of tasks) {
    const heading = task.group?.trim() || 'Tasks'
    const iconName = task.icon?.trim() || 'terminal'
    const entry: Command = {
      id: `task:${task.id}`,
      label: task.label,
      lucideIcon: iconName,
      keywords: ['task', task.label, task.id, task.group ?? ''],
      onSelect: () => void openTaskTab(projectId, task)
    }
    const existing = buckets.get(heading)
    if (existing) existing.push(entry)
    else buckets.set(heading, [entry])
  }

  return Array.from(buckets, ([heading, commands]) => ({ heading, commands }))
}
