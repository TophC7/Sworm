// Shared command actions.
//
// Used by both the global shortcut wiring (shortcuts.svelte.ts) and the
// Command Center palette (commands/*.svelte.ts). Keeping the action
// bodies here guarantees the two entry points stay equivalent — press
// Ctrl+N and click "New File" from the palette run the same code path.

import { confirmAsync } from '$lib/stores/confirmService.svelte'
import { getDirtyEditorsCount, hasAnyDirtyEditors } from '$lib/stores/dirtyEditors.svelte'
import { notify } from '$lib/stores/notifications.svelte'
import { createAndOpenSession } from '$lib/stores/sessions.svelte'
import { getConnectedProviders } from '$lib/stores/providers.svelte'
import { showProjectPicker } from '$lib/stores/ui.svelte'
import {
  addNewEmptyEditorTab,
  flushPersistencePending,
  getActiveProjectId,
  reopenLastClosedTab
} from '$lib/stores/workspace.svelte'
import { closeFocusedTab } from '$lib/utils/tabActions.svelte'
import { runNotifiedTask, getErrorMessage } from '$lib/utils/notifiedTask'

/** Managed reload: confirm unsaved, flush persistence, then reload. */
export async function reloadView(): Promise<void> {
  if (hasAnyDirtyEditors()) {
    const count = getDirtyEditorsCount()
    const noun = count === 1 ? 'file' : 'files'
    const proceed = await confirmAsync({
      title: 'Unsaved changes',
      message: `You have ${count} unsaved ${noun}. Reload and lose changes?`,
      confirmLabel: 'Reload',
      cancelLabel: 'Keep editing'
    })
    if (!proceed) return
  }
  try {
    await flushPersistencePending()
  } catch (error) {
    console.warn('Reload flush failed:', error)
  }
  window.location.reload()
}

export function newEmptyFile(): void {
  const projectId = getActiveProjectId()
  if (!projectId) return
  addNewEmptyEditorTab(projectId)
}

export async function newTerminalSession(): Promise<void> {
  const projectId = getActiveProjectId()
  if (!projectId) return
  const connected = getConnectedProviders()
  if (!connected.some((p) => p.id === 'terminal')) {
    notify.error('Terminal unavailable', 'Connect the Terminal provider in Settings first.')
    return
  }
  await runNotifiedTask(() => createAndOpenSession(projectId, 'terminal', 'Terminal session'), {
    loading: { title: 'Starting Terminal session' },
    error: { title: 'Failed to start Terminal session' }
  })
}

export async function closeActiveTab(): Promise<void> {
  const projectId = getActiveProjectId()
  if (!projectId) return
  try {
    await closeFocusedTab(projectId)
  } catch (error) {
    notify.error('Close tab failed', getErrorMessage(error))
  }
}

export function reopenTab(): void {
  const projectId = getActiveProjectId()
  if (!projectId) return
  reopenLastClosedTab(projectId)
}

export function openProjectPicker(): void {
  showProjectPicker()
}
