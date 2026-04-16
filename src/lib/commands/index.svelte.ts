import type { CommandGroup, FileCallbacks } from './types'
import { getFileCommands } from './file.svelte'
import { getSessionCommands } from './sessions.svelte'
import { getGitCommands } from './git.svelte'
import { getViewCommands } from './view.svelte'
import { getEditorCommands } from './editor.svelte'

export type { Command, CommandConfirm, CommandGroup, FileCallbacks } from './types'

/**
 * App-level command groups (shown when search has no `>` prefix).
 * Called inside $derived so reactive reads in each module are tracked.
 */
export function getAppCommandGroups(callbacks: FileCallbacks): CommandGroup[] {
  return [...getFileCommands(callbacks), ...getSessionCommands(), ...getGitCommands(), ...getViewCommands()].filter(
    (g) => g.commands.length > 0
  )
}

/**
 * Editor command groups (shown when search starts with `>`).
 * Only populated when an editor tab is/was recently focused.
 */
export function getEditorCommandGroups(): CommandGroup[] {
  return getEditorCommands().filter((g) => g.commands.length > 0)
}
