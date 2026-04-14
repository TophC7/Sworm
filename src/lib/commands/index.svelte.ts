import type { CommandGroup, FileCallbacks } from './types'
import { getFileCommands } from './file.svelte'
import { getSessionCommands } from './sessions.svelte'
import { getGitCommands } from './git.svelte'
import { getViewCommands } from './view.svelte'

export type { Command, CommandConfirm, CommandGroup, FileCallbacks } from './types'

/**
 * Aggregates all command groups.
 * Called inside $derived so reactive reads in each module are tracked.
 */
export function getCommandGroups(callbacks: FileCallbacks): CommandGroup[] {
  return [...getFileCommands(callbacks), ...getSessionCommands(), ...getGitCommands(), ...getViewCommands()].filter(
    (g) => g.commands.length > 0
  )
}
