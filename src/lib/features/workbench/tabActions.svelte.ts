// Shared tab-close flow.
//
// Lifted out of the tab strip so a keyboard shortcut (Ctrl+W) runs the
// same dirty-check + PTY-stop path as clicking a tab's close button. Keep
// this the single source of truth — divergence between the two call sites
// is how tabs end up closed while their PTY is still alive.

import { stopTaskProcess } from '$lib/features/tasks/service.svelte'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { stopSessionProcess } from '$lib/features/sessions/service.svelte'
import { isProcessLive, type TabId } from '$lib/features/workbench/model'
import { closeTab, getActiveTab, getTabs, releaseClosedWorktreeFolders } from '$lib/features/workbench/state.svelte'
import {
  clearTextSurfaceDirty,
  discardTextSurfaceBuffer,
  isTextSurfaceDirty,
  setTextBaseVersion
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { getErrorMessage } from '$lib/utils/client-error'
import { isPathWithin } from '$lib/utils/paths'
import { isTabInert } from '$lib/features/workbench/groups.svelte'
import { textModelRevision, type TextModelRevision } from '$lib/features/editor/renderers/monaco/text/modelCache'

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
export async function closeTabWithChecks(
  tabId: TabId,
  options: { strict?: boolean; unsavedRevision?: TextModelRevision | null } = {}
): Promise<boolean> {
  const tab = getTabs().find((t) => t.id === tabId)
  if (!tab) return false
  if (tab.locked || isTabInert(tab)) return false

  const textDirty = tab.kind === 'text' && isTextSurfaceDirty(tab.id)
  if (textDirty) {
    let revision = textModelRevision(tab)
    const consent = options.unsavedRevision
    if (!revision || !consent || revision.model !== consent.model || revision.version !== consent.version) {
      for (;;) {
        const proceed = await confirmAsync({
          title: 'Unsaved changes',
          message: `${tab.fileName} has unsaved changes. Close and lose them?`,
          confirmLabel: 'Close',
          cancelLabel: 'Keep editing'
        })
        if (!proceed) return false
        const current = textModelRevision(tab)
        if (
          !options.strict ||
          !revision ||
          (current !== null && current.model === revision.model && current.version === revision.version)
        )
          break
        revision = current
      }
    }
  }

  let current = getTabs().find((candidate) => candidate.id === tabId)
  if (
    !current ||
    current.folderPath !== tab.folderPath ||
    current.kind !== tab.kind ||
    current.locked ||
    isTabInert(current)
  )
    return false

  if (tab.kind === 'session' && (tab.runId !== null || isProcessLive(tab.status))) {
    try {
      await stopSessionProcess(tab.id)
    } catch (err) {
      notify.error('Stop session failed', getErrorMessage(err))
      return false
    }
  }

  if (tab.kind === 'task') {
    // Stop even a detached or completed run to release its retained state.
    // Ordinary close reports stop errors; destructive worktree closure must abort.
    try {
      await stopTaskProcess(tab)
    } catch (err) {
      notify.error('Stop task failed', getErrorMessage(err))
      if (options.strict) return false
    }
  }
  current = getTabs().find((candidate) => candidate.id === tabId)
  if (
    !current ||
    current.folderPath !== tab.folderPath ||
    current.kind !== tab.kind ||
    current.locked ||
    isTabInert(current)
  )
    return false

  if (tab.kind === 'text') {
    if (textDirty || tab.filePath == null) {
      discardTextSurfaceBuffer(tab)
    } else {
      setTextBaseVersion(tab.folderPath, tab.filePath, null)
    }
  }

  // `closeTab` disposes the session/task manager for the tab.
  closeTab(tabId)
  if (tab.kind === 'text') clearTextSurfaceDirty(tab.id)
  return !getTabs().some((current) => current.id === tabId)
}

/** Close only these checkouts, with all cancellation checks before stopping any run. */
export async function closeWorktreeTabs(roots: string[]): Promise<boolean> {
  const affected = getTabs().filter((tab) => roots.some((root) => isPathWithin(root, tab.folderPath)))
  const folderPaths = [...new Set([...roots, ...affected.map((tab) => tab.folderPath)])]
  if (affected.some((tab) => tab.locked || isTabInert(tab))) {
    notify.warning(
      'Worktree shutdown blocked',
      'Unlock affected tabs and finish any workbench transfer before continuing.'
    )
    return false
  }
  const unsavedConsent = new Map<TabId, TextModelRevision | null>()
  for (const tab of affected) {
    if (tab.kind !== 'text' || !isTextSurfaceDirty(tab.id)) continue
    const revision = textModelRevision(tab)
    const proceed = await confirmAsync({
      title: 'Unsaved changes',
      message: `${tab.fileName} has unsaved changes. Close and lose them?`,
      confirmLabel: 'Close',
      cancelLabel: 'Keep editing'
    })
    if (!proceed) return false
    unsavedConsent.set(tab.id, revision)
  }
  let closed = 0
  try {
    for (const tab of affected) {
      if (!getTabs().some((current) => current.id === tab.id)) continue
      if (!(await closeTabWithChecks(tab.id, { strict: true, unsavedRevision: unsavedConsent.get(tab.id) }))) {
        throw new Error('A tab could not be stopped or closed.')
      }
      closed += 1
    }
    await releaseClosedWorktreeFolders(roots, folderPaths)
    return true
  } catch (error) {
    notify.error(
      'Worktree shutdown failed',
      `${getErrorMessage(error)}${closed > 0 ? ` ${closed} tab(s) already closed; stopped commands were not restarted.` : ''} The checkout was not deleted or migrated.`
    )
    return false
  }
}

/** Close the active tab. Safe no-op when there is none. Used by Ctrl+W. */
export async function closeFocusedTab(): Promise<boolean> {
  const active = getActiveTab()
  if (!active) return false
  return closeTabWithChecks(active.id)
}
