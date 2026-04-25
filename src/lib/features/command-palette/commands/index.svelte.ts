import type { CommandGroup } from './types'
import { getProjects } from '$lib/features/projects/state.svelte'
import { getOpenProjectIds, openProject } from '$lib/features/workbench/state.svelte'
import { getEditorCommands } from './editor.svelte'
import { getTaskPaletteGroups } from './tasks.svelte'
import { getVisibleAppPaletteCommands, toPaletteCommand } from './registry.svelte'
import { FolderClockIcon } from '$lib/icons/lucideExports'

export type { Command, CommandGroup } from './types'

function getRecentProjectGroups(): CommandGroup[] {
  const openIds = getOpenProjectIds()
  const recent = getProjects().filter((project) => !openIds.includes(project.id))
  if (recent.length === 0) return []
  return [
    {
      heading: 'Recent Projects',
      commands: recent.map((project) => ({
        id: `recent-${project.id}`,
        label: project.name,
        subtitle: project.path,
        icon: FolderClockIcon,
        keywords: [project.name, project.path],
        onSelect: () => openProject(project.id)
      }))
    }
  ]
}

export function getAppCommandGroups(): CommandGroup[] {
  const visibleDefinitions = getVisibleAppPaletteCommands()
  const groups = new Map<string, CommandGroup>()

  for (const definition of visibleDefinitions) {
    const command = toPaletteCommand(definition)
    const existing = groups.get(definition.group)
    if (existing) existing.commands.push(command)
    else groups.set(definition.group, { heading: definition.group, commands: [command] })
  }

  return [...Array.from(groups.values()), ...getRecentProjectGroups()].filter((group) => group.commands.length > 0)
}

export function getEditorCommandGroups(): CommandGroup[] {
  return getEditorCommands().filter((g) => g.commands.length > 0)
}

export function getTaskCommandGroups(): CommandGroup[] {
  return getTaskPaletteGroups().filter((g) => g.commands.length > 0)
}
