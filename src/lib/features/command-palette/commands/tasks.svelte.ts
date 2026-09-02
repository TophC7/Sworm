import type { Command, CommandGroup } from './types'
import { getTasksReactive } from '$lib/features/tasks/state.svelte'
import { openTaskTab } from '$lib/features/tasks/service.svelte'
import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'

export function getTaskPaletteGroups(): CommandGroup[] {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return []

  const tasks = getTasksReactive(folderPath)
  if (tasks.length === 0) return []

  const buckets = new Map<string, Command[]>()
  for (const task of tasks) {
    const heading = task.group?.trim() || 'Tasks'
    const iconName = task.icon?.trim() || 'terminal'
    const entry: Command = {
      id: `task:${task.id}`,
      label: task.label,
      lucideIcon: iconName,
      keywords: ['task', task.label, task.id, task.group ?? ''],
      onSelect: () => void openTaskTab(folderPath, task)
    }
    const existing = buckets.get(heading)
    if (existing) existing.push(entry)
    else buckets.set(heading, [entry])
  }

  return Array.from(buckets, ([heading, commands]) => ({ heading, commands }))
}
