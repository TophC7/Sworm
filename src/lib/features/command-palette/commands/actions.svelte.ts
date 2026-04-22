// Shared command actions.
//
// Used by both the global shortcut wiring (shortcuts.svelte.ts) and the
// Command Center palette (commands/*.svelte.ts). Keeping the action
// bodies here guarantees the two entry points stay equivalent — press
// Ctrl+N and click "New File" from the palette run the same code path.

import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { createAndOpenSession } from '$lib/features/sessions/state/sessions.svelte'
import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
import { showProjectPicker } from '$lib/features/app-shell/project-picker/state.svelte'
import {
  createUntitledTextSurface,
  getDirtyTextSurfaceCount,
  hasAnyDirtyTextSurfaces
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { flushPersistencePending, getActiveProjectId, reopenLastClosedTab } from '$lib/features/workbench/state.svelte'
import { closeFocusedTab } from '$lib/features/workbench/tabActions.svelte'
import { runNotifiedTask, getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

/** Managed reload: confirm unsaved, flush persistence, then reload. */
export async function reloadView(): Promise<void> {
  if (hasAnyDirtyTextSurfaces()) {
    const count = getDirtyTextSurfaceCount()
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

export async function newEmptyFile(): Promise<void> {
  const projectId = getActiveProjectId()
  if (!projectId) return
  await createUntitledTextSurface(projectId)
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

export async function reopenTab(): Promise<void> {
  const projectId = getActiveProjectId()
  if (!projectId) return
  await reopenLastClosedTab(projectId)
}

export function openProjectPicker(): void {
  showProjectPicker()
}
