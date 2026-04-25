// Global keyboard shortcuts.
//
// Every stable app command registers here by id, even when it has no
// default keybinding. That lets a user add a shortcut later
// without needing a second dispatch path.

import {
  installKeybindingListener,
  registerGlobalBinding
} from '$lib/features/command-palette/shortcuts/keybindings.svelte'
import { toggleCommandPalette } from '$lib/features/command-palette/state.svelte'
import { getAppShortcutCommands, terminalPolicyOptions } from '$lib/features/command-palette/commands/registry.svelte'
import { loadShortcutOverrides } from '$lib/features/command-palette/shortcuts/overrides.svelte'

export function setupGlobalShortcuts(): () => void {
  const disposers: Array<() => void> = []
  void loadShortcutOverrides()

  for (const command of getAppShortcutCommands()) {
    const handler = command.id === 'toggle-command-palette' ? toggleCommandPalette : command.run
    if (!handler) continue
    disposers.push(
      registerGlobalBinding(
        command.id,
        command.defaultKeybindings,
        () => {
          void handler()
        },
        terminalPolicyOptions(command.terminalPolicy)
      )
    )
  }

  const uninstall = installKeybindingListener()
  return () => {
    uninstall()
    for (const dispose of disposers) dispose()
  }
}
