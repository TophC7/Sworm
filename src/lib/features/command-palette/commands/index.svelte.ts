import type { CommandGroup, FileCallbacks } from './types'
import { getFileCommands } from './file.svelte'
import { getSessionCommands } from './sessions.svelte'
import { getGitCommands } from './git.svelte'
import { getTaskAppCommands, getTaskPaletteGroups } from './tasks.svelte'
import { getViewCommands } from './view.svelte'
import { getNotificationCommands } from './notifications.svelte'
import { getEditorCommands } from './editor.svelte'

export type { Command, CommandConfirm, CommandGroup, FileCallbacks } from './types'

/**
 * App-level command groups (shown when search has no `>` prefix).
 * Called inside $derived so reactive reads in each module are tracked.
 */
export function getAppCommandGroups(callbacks: FileCallbacks): CommandGroup[] {
  return [
    ...getFileCommands(callbacks),
    ...getSessionCommands(),
    ...getTaskAppCommands(),
    ...getGitCommands(),
    ...getViewCommands(),
    ...getNotificationCommands()
  ].filter((g) => g.commands.length > 0)
}

/**
 * Editor command groups (shown when search starts with `>`).
 * Only populated when an editor tab is/was recently focused.
 */
export function getEditorCommandGroups(): CommandGroup[] {
  return getEditorCommands().filter((g) => g.commands.length > 0)
}

/**
 * Task palette groups (shown when search starts with `!`). Returns
 * one group per task `group` field, with an ungrouped "Tasks" bucket
 * for entries that don't specify one.
 */
export function getTaskCommandGroups(): CommandGroup[] {
  return getTaskPaletteGroups().filter((g) => g.commands.length > 0)
}
