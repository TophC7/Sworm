// Shared tab-close flow.
//
// Lifted out of the tab strip so a keyboard shortcut (Ctrl+W) runs the
// same dirty-check + PTY-stop path as clicking a tab's close button. Keep
// this the single source of truth — divergence between the two call sites
// is how tabs end up closed while their PTY is still alive.

import { backend } from '$lib/api/backend'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { stopSessionProcess } from '$lib/features/sessions/service.svelte'
import { isProcessLive, type TabId } from '$lib/features/workbench/model'
import { closeTab, getActiveTab, getTabs } from '$lib/features/workbench/state.svelte'
import {
  clearTextSurfaceDirty,
  discardTextSurfaceBuffer,
  isTextSurfaceDirty
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

/**
 * Close a specific tab with full safety checks:
 *  - prompt on unsaved editor buffers
 *  - stop the PTY for live session tabs (aborts close if stop fails —
 *    leaking a running PTY while the tab vanishes would leave the
 *    process hidden until app quit)
 *
 * Returns true when the tab was closed, false when the user cancelled
 * or the close could not proceed.
 */
export async function closeTabWithChecks(tabId: TabId): Promise<boolean> {
  const tab = getTabs().find((t) => t.id === tabId)
  if (!tab) return false
  if (tab.locked) return false

  const textDirty = tab.kind === 'text' && isTextSurfaceDirty(tab.id)
  if (textDirty) {
    const proceed = await confirmAsync({
      title: 'Unsaved changes',
      message: `${tab.fileName} has unsaved changes. Close and lose them?`,
      confirmLabel: 'Close',
      cancelLabel: 'Keep editing'
    })
    if (!proceed) return false
  }

  if (tab.kind === 'session' && isProcessLive(tab.status)) {
    try {
      await stopSessionProcess(tab.sessionId)
    } catch (err) {
      notify.error('Stop session failed', getErrorMessage(err))
      return false
    }
  }

  if (tab.kind === 'task' && isProcessLive(tab.status)) {
    // Stop failures shouldn't block the close — if the PTY is already
    // gone the backend swallows the error; any real error surfaces as
    // a toast but we still tear down the tab to avoid orphaning it.
    try {
      await backend.tasks.stop(tab.runId)
    } catch (err) {
      notify.error('Stop task failed', getErrorMessage(err))
    }
  }

  if (tab.kind === 'text' && (textDirty || tab.filePath == null)) {
    discardTextSurfaceBuffer(tab)
  }

  // `closeTab` disposes the session/task manager for the tab.
  closeTab(tabId)
  if (tab.kind === 'text') clearTextSurfaceDirty(tab.id)
  return true
}

/** Close the active tab. Safe no-op when there is none. Used by Ctrl+W. */
export async function closeFocusedTab(): Promise<boolean> {
  const active = getActiveTab()
  if (!active) return false
  return closeTabWithChecks(active.id)
}
