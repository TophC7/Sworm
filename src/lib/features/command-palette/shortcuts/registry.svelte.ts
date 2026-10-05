import { untrack } from 'svelte'
import {
  getAppCommandDefinitions,
  getAppShortcutCommands,
  getEditorShortcutCommands,
  type ShortcutCommandDefinition
} from '$lib/features/command-palette/commands/registry.svelte'
import { getEffectiveBindings } from '$lib/features/command-palette/shortcuts/overrides.svelte'
import { normalizeShortcut } from '$lib/features/command-palette/shortcuts/spec'

export interface ShortcutCommandInfo extends ShortcutCommandDefinition {
  effectiveKeybindings: string[]
}

export interface ShortcutConflict {
  command: ShortcutCommandInfo
  key: string
}

export function getCommandShortcut(id: string): string | undefined {
  return getEffectiveBindings(
    id,
    untrack(() => getAppCommandDefinitions().find((command) => command.id === id)?.defaultKeybindings) ?? []
  )[0]
}

function getShortcutCommands(): ShortcutCommandDefinition[] {
  return [...getAppShortcutCommands(), ...getEditorShortcutCommands()]
}

function getShortcutCommandInfo(command: ShortcutCommandDefinition): ShortcutCommandInfo {
  return {
    ...command,
    effectiveKeybindings: getEffectiveBindings(command.id, command.defaultKeybindings)
  }
}

export function getShortcutInfos(): ShortcutCommandInfo[] {
  return getShortcutCommands().map(getShortcutCommandInfo)
}

export function findShortcutConflict(
  commandId: string,
  key: string,
  infos = getShortcutInfos()
): ShortcutConflict | null {
  const normalized = normalizeShortcut(key)
  if (!normalized) return null
  for (const command of infos) {
    if (command.id === commandId) continue
    if (command.effectiveKeybindings.some((binding) => normalizeShortcut(binding) === normalized)) {
      return {
        command,
        key: command.effectiveKeybindings.find((binding) => normalizeShortcut(binding) === normalized) ?? key
      }
    }
  }
  return null
}
