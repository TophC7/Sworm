// App-menu model for the titlebar hamburger (TitleBarMenu). Reads
// reactive getters; call it from a `$derived` so the structure stays live.

import { getAppCommandDefinitions } from '$lib/features/command-palette/commands/registry.svelte'
import { getEffectiveBindings } from '$lib/features/command-palette/shortcuts/overrides.svelte'
import { formatShortcut } from '$lib/features/command-palette/shortcuts/spec'

export interface MenuItem {
  kind: 'item'
  label: string
  onSelect: () => void
  shortcut?: string
  disabled?: boolean
}

export interface MenuSeparator {
  kind: 'separator'
}

export type MenuEntry = MenuItem | MenuSeparator

const MENU_ORDER = [
  'new-window',
  'reopen-closed-tab',
  'close-workbench',
  null,
  'reveal-in-file-manager',
  'open-in-external-terminal',
  'open-folder-settings',
  'settings',
  null,
  'toggle-sidebar',
  'zoom-in',
  'zoom-out',
  'zoom-reset'
] as const

const MENU_LABELS: Partial<Record<string, string>> = {
  'reveal-in-file-manager': 'Reveal Folder in File Manager',
  'open-in-external-terminal': 'Open Folder in External Terminal',
  'open-folder-settings': 'Folder Settings…',
  settings: 'Settings…'
}

export function buildAppMenu(): MenuEntry[] {
  const definitions = new Map(getAppCommandDefinitions().map((definition) => [definition.id, definition]))
  const entries: MenuEntry[] = []
  for (const id of MENU_ORDER) {
    if (id === null) {
      entries.push({ kind: 'separator' })
      continue
    }
    const definition = definitions.get(id)
    if (!definition) continue
    entries.push({
      kind: 'item',
      label: MENU_LABELS[id] ?? (typeof definition.label === 'function' ? definition.label() : definition.label),
      disabled: !(definition.visible?.() ?? true),
      onSelect: () => void definition.run(),
      shortcut:
        id === 'new-window' || id === 'settings'
          ? formatShortcut(getEffectiveBindings(id, definition.defaultKeybindings ?? [])[0])
          : undefined
    })
  }
  return entries
}
