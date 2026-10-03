import type { CommandGroup } from './types'
import { getEditorCommands } from './editor.svelte'
import { getTaskPaletteGroups } from './tasks.svelte'
import { getVisibleAppPaletteCommands, toPaletteCommand } from './registry.svelte'

export type { Command, CommandGroup } from './types'

export function getAppCommandGroups(): CommandGroup[] {
  const groups = new Map<string, CommandGroup>()
  for (const definition of getVisibleAppPaletteCommands()) {
    const command = toPaletteCommand(definition)
    const existing = groups.get(definition.group)
    if (existing) existing.commands.push(command)
    else groups.set(definition.group, { heading: definition.group, commands: [command] })
  }
  return [...groups.values()].filter((group) => group.commands.length > 0)
}

export function getEditorCommandGroups(): CommandGroup[] {
  return getEditorCommands().filter((group) => group.commands.length > 0)
}

export function getTaskCommandGroups(): CommandGroup[] {
  return getTaskPaletteGroups().filter((group) => group.commands.length > 0)
}
