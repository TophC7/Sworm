// Shared tab-close flow.
//
// Previously inlined in PaneTabBar. Lifted so a keyboard shortcut
// (Ctrl+W) can run the same dirty-check + PTY-stop path as clicking
// the tab's close button. Keep this the single source of truth —
// divergence between the two call sites is how tabs end up closed while
// their PTY is still alive.

import { backend } from '$lib/api/backend'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getSessions, updateSessionInList } from '$lib/features/sessions/state/sessions.svelte'
import type { TabId } from '$lib/features/workbench/state.svelte'
import { closeTab, collapsePaneIfEmpty, getAllTabs, getFocusedTab } from '$lib/features/workbench/state.svelte'
import {
  clearTextSurfaceDirty,
  discardTextSurfaceBuffer,
  isTextSurfaceDirty
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

/**
 * Close a specific tab with full safety checks:
 *  - prompt on unsaved editor buffers
 *  - stop the PTY for running session tabs (aborts close if stop fails
 *    — leaking a running PTY while the tab vanishes would leave the
 *    process hidden until app quit)
 *  - collapse the host pane if this was its last tab
 *
 * Returns true when the tab was closed, false when the user cancelled
 * or the close could not proceed.
 */
export async function closeTabWithChecks(projectId: string, tabId: TabId): Promise<boolean> {
  const tabs = getAllTabs(projectId)
  const tab = tabs.find((t) => t.id === tabId)
  if (!tab) return false
  if (tab.locked) return false

  const textDirty = tab.kind === 'text' && isTextSurfaceDirty(projectId, tab.id)
  if (textDirty) {
    const proceed = await confirmAsync({
      title: 'Unsaved changes',
      message: `${tab.fileName} has unsaved changes. Close and lose them?`,
      confirmLabel: 'Close',
      cancelLabel: 'Keep editing'
    })
    if (!proceed) return false
  }

  if (tab.kind === 'session') {
    const sessions = getSessions(projectId)
    const session = sessions.find((s) => s.id === tab.sessionId)
    if (session && (session.status === 'running' || session.status === 'starting')) {
      try {
        await backend.sessions.stop(tab.sessionId)
        updateSessionInList(projectId, tab.sessionId, { status: 'stopped' })
      } catch (err) {
        notify.error('Stop session failed', getErrorMessage(err))
        return false
      }
    }
  }

  if (tab.kind === 'task' && (tab.status === 'running' || tab.status === 'starting')) {
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
    discardTextSurfaceBuffer(projectId, tab)
  }

  closeTab(projectId, tabId)
  if (tab.kind === 'text') clearTextSurfaceDirty(projectId, tab.id)
  collapsePaneIfEmpty(projectId)
  return true
}

/**
 * Close the active tab in the focused pane. Safe no-op when nothing is
 * focused or the pane is empty. Used by Ctrl+W.
 */
export async function closeFocusedTab(projectId: string): Promise<boolean> {
  const focused = getFocusedTab(projectId)
  if (!focused) return false
  return closeTabWithChecks(projectId, focused.id)
}
