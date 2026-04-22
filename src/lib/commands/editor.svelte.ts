// Editor command group — surfaces Monaco actions in the app command palette.
//
// Only returns commands when an editor tab is focused (the actions
// store is populated on editor focus and cleared on blur/unmount).

import type { CommandGroup } from './types'
import { getTextEditorActions } from '$lib/renderers/monaco/text/actions.svelte'

export function getEditorCommands(): CommandGroup[] {
  const actions = getTextEditorActions()
  if (actions.length === 0) return []

  return [
    {
      heading: 'Editor',
      commands: actions.map((a) => ({
        id: `editor:${a.id}`,
        label: a.label,
        keywords: a.id.split('.'),
        onSelect: a.run
      }))
    }
  ]
}
