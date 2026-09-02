import type { CommandGroup } from './types'
import { getRecentUnopenedFolders, openFolder } from '$lib/features/workbench/state.svelte'
import { basename } from '$lib/utils/paths'
import { getEditorCommands } from './editor.svelte'
import { getTaskPaletteGroups } from './tasks.svelte'
import { getVisibleAppPaletteCommands, toPaletteCommand } from './registry.svelte'
import { FolderClockIcon } from '$lib/icons/lucideExports'

export type { Command, CommandGroup } from './types'

function getRecentFolderGroups(): CommandGroup[] {
  const recent = getRecentUnopenedFolders()
  if (recent.length === 0) return []
  return [
    {
      heading: 'Recent Folders',
      commands: recent.map((path) => ({
        id: `recent-${path}`,
        label: basename(path),
        subtitle: path,
        icon: FolderClockIcon,
        keywords: [basename(path), path],
        onSelect: () => openFolder(path)
      }))
    }
  ]
}

export function getAppCommandGroups(): CommandGroup[] {
  const groups = new Map<string, CommandGroup>()
  for (const definition of getVisibleAppPaletteCommands()) {
    const command = toPaletteCommand(definition)
    const existing = groups.get(definition.group)
    if (existing) existing.commands.push(command)
    else groups.set(definition.group, { heading: definition.group, commands: [command] })
  }
  return [...groups.values(), ...getRecentFolderGroups()].filter((group) => group.commands.length > 0)
}

export function getEditorCommandGroups(): CommandGroup[] {
  return getEditorCommands().filter((group) => group.commands.length > 0)
}

export function getTaskCommandGroups(): CommandGroup[] {
  return getTaskPaletteGroups().filter((group) => group.commands.length > 0)
}
