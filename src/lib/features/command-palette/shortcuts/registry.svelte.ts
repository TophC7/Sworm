import {
  getAppShortcutCommand,
  getAppShortcutCommands,
  type ShortcutCommandDefinition
} from '$lib/features/command-palette/commands/registry.svelte'
import { getEffectiveBindings } from '$lib/features/command-palette/shortcuts/overrides.svelte'
import { normalizeShortcut } from '$lib/features/command-palette/shortcuts/spec'
import { getTextEditorActions } from '$lib/features/editor/renderers/monaco/text/actions.svelte'

export interface ShortcutCommandInfo extends ShortcutCommandDefinition {
  effectiveKeybindings: string[]
}

export interface ShortcutConflict {
  command: ShortcutCommandInfo
  key: string
}

export function getEditorShortcutCommands(): ShortcutCommandDefinition[] {
  return getTextEditorActions().map((action) => ({
    id: `editor:${action.id}`,
    label: action.label,
    group: 'Editor',
    keywords: action.id.split('.'),
    source: 'editor',
    defaultKeybindings: action.defaultKeybindings,
    run: action.run
  }))
}

export function getShortcutCommands(): ShortcutCommandDefinition[] {
  return [...getAppShortcutCommands(), ...getEditorShortcutCommands()]
}

export function getShortcutCommand(id: string): ShortcutCommandDefinition | null {
  if (id.startsWith('editor:')) return getEditorShortcutCommands().find((command) => command.id === id) ?? null
  return getAppShortcutCommand(id)
}

export function getShortcutCommandInfo(command: ShortcutCommandDefinition): ShortcutCommandInfo {
  return {
    ...command,
    effectiveKeybindings: getEffectiveBindings(command.id, command.defaultKeybindings)
  }
}

export function getShortcutInfos(): ShortcutCommandInfo[] {
  return getShortcutCommands().map(getShortcutCommandInfo)
}

export function findShortcutConflict(commandId: string, key: string): ShortcutConflict | null {
  const normalized = normalizeShortcut(key)
  if (!normalized) return null
  for (const command of getShortcutInfos()) {
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
