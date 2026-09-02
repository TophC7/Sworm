// Shared app actions.
//
// Button clicks, command-palette entries, and global shortcuts all call
// these functions so confirmation and side effects stay on one path.

import { backend } from '$lib/api/backend'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
import { startSession } from '$lib/features/sessions/service.svelte'
import { setSettingsOpen } from '$lib/features/settings/dialog/state.svelte'
import { getLastTaskId, rerunLastTask } from '$lib/features/tasks/service.svelte'
import { openCommandPaletteWithSearch } from '$lib/features/command-palette/state.svelte'
import {
  createUntitledTextSurface,
  getDirtyTextSurfaceCount,
  hasAnyDirtyTextSurfaces,
  openTextFile
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { flushWorkbench } from '$lib/features/workbench/persistence'
import {
  getActiveFolderPath,
  hasRunningSessionInFolder,
  openFolder,
  reopenLastClosedTab
} from '$lib/features/workbench/state.svelte'
import { closeFocusedTab } from '$lib/features/workbench/tabActions.svelte'
import { revealItemInDir } from '@tauri-apps/plugin-opener'

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
    await flushWorkbench()
  } catch (error) {
    console.warn('Reload flush failed:', error)
  }
  window.location.reload()
}

export function newEmptyFile(): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  createUntitledTextSurface(folderPath)
}

/** Native directory picker → open (or focus) that folder. */
export async function openFolderPicker(): Promise<void> {
  try {
    const path = await backend.folders.selectDirectory()
    if (path) await openFolder(path)
  } catch (error) {
    notify.error('Open folder failed', getErrorMessage(error))
  }
}

export function openSettings(): void {
  setSettingsOpen(true)
}

export async function openGlobalSettingsFile(): Promise<void> {
  try {
    await backend.settings.openGlobalFile()
  } catch (error) {
    notify.error('Open Global Settings failed', getErrorMessage(error))
  }
}

export async function openFolderSettingsFile(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) {
    notify.error('No active folder', 'Open a folder before opening folder settings.')
    return
  }

  try {
    await backend.settings.openProjectFile(folderPath)
    await openTextFile(folderPath, '.sworm/settings.jsonc', { temporary: false })
  } catch (error) {
    notify.error('Open Folder Settings failed', getErrorMessage(error))
  }
}

export function revealActiveFolderInFileManager(): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  void revealItemInDir(folderPath).catch((error) => {
    notify.error('Reveal in file manager failed', getErrorMessage(error))
  })
}

export function openActiveFolderInExternalTerminal(): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  void backend.folders.openInTerminal(folderPath).catch((error) => {
    notify.error('Open in terminal failed', getErrorMessage(error))
  })
}

export async function createSessionWithSharedWorkspaceWarning(providerId: string, label: string): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  if (!getConnectedProviders().some((p) => p.id === providerId)) {
    notify.error(`${label} unavailable`, `Connect the ${label} provider in Settings first.`)
    return
  }

  if (hasRunningSessionInFolder(folderPath)) {
    const proceed = await confirmAsync({
      title: 'Shared Workspace Warning',
      message:
        'Another session is already running in this folder.\n\n' +
        'Sessions in the same folder share the same working tree and branch.\n' +
        'Changes made by one session may conflict with another.',
      confirmLabel: 'Start Anyway',
      cancelLabel: 'Cancel'
    })
    if (!proceed) return
  }

  startSession(folderPath, providerId, `${label} session`)
}

export async function newTerminalSession(): Promise<void> {
  await createSessionWithSharedWorkspaceWarning('terminal', 'Terminal')
}

export function showTasks(): void {
  openCommandPaletteWithSearch('! ')
}

/**
 * Opens the command palette in file-search mode. Bound to Ctrl+P,
 * matching VS Code's Quick Open.
 */
export function showFiles(): void {
  openCommandPaletteWithSearch('/ ')
}

export async function rerunLastFolderTask(): Promise<void> {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  if (!getLastTaskId(folderPath)) return
  const tabId = await rerunLastTask(folderPath)
  if (tabId === null) {
    notify.error('Cannot re-run task', 'The last task is no longer defined in .sworm/tasks.json')
  }
}

export async function closeActiveTab(): Promise<void> {
  try {
    await closeFocusedTab()
  } catch (error) {
    notify.error('Close tab failed', getErrorMessage(error))
  }
}

export function reopenTab(): void {
  reopenLastClosedTab()
}
