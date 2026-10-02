// Shared app actions.
//
// Button clicks, command-palette entries, and global shortcuts all call
// these functions so confirmation and side effects stay on one path.

import { backend } from '$lib/api/backend'
import { platform, requireNative } from '$lib/platform'
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
  isTextSurfaceDirty,
  openTextFile
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { flushWorkbench } from '$lib/features/workbench/persistence'
import {
  getActiveFolderPath,
  getTabs,
  getWorkbenchId,
  openFolder,
  reopenLastClosedTab
} from '$lib/features/workbench/state.svelte'
import { closeFocusedTab } from '$lib/features/workbench/tabActions.svelte'
import { basename, resolveProjectFile, splitRemotePath } from '$lib/utils/paths'

/** Managed reload: confirm unsaved, flush persistence, then reload. */
export async function reloadView(): Promise<void> {
  // Web: the document's beforeunload guard owns the dirty prompt.
  if (platform.native && hasAnyDirtyTextSurfaces()) {
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
    await flushWorkbench(getWorkbenchId())
  } catch (error) {
    console.warn('Reload flush failed:', error)
  }
  window.location.reload()
}

export function confirmCloseWorkbench(folders: string[]): Promise<boolean> {
  return confirmAsync({
    title: 'Close Workbench',
    message: `Stop all sessions and tasks in ${[...new Set(folders.map(basename))].join(', ') || 'this workbench'} and delete its saved tabs and layout? Unsaved changes in any connected page will be lost.`,
    confirmLabel: 'Close Workbench',
    cancelLabel: 'Cancel'
  })
}

let closingWorkbench = false

/** Confirm, then ask the host to close (stop runs, delete state of) this workbench. */
export async function closeCurrentWorkbench(): Promise<void> {
  if (closingWorkbench) return
  closingWorkbench = true
  try {
    const closeCurrent = platform.workbench.closeCurrent
    if (!closeCurrent) throw new Error('Closing the current workbench is not supported here')
    const dirtyPaths = getTabs().flatMap((tab) =>
      tab.kind === 'text' && isTextSurfaceDirty(tab.id)
        ? [tab.filePath === null ? tab.fileName : resolveProjectFile(tab.folderPath, tab.filePath)]
        : []
    )
    const confirmed = await confirmAsync({
      title: 'Close Workbench',
      message: [
        'Stop all sessions and tasks in this workbench and delete its saved tabs and layout?',
        ...(dirtyPaths.length > 0 ? ['', 'Unsaved changes will be lost:', ...dirtyPaths] : [])
      ].join('\n'),
      confirmLabel: 'Close Workbench',
      cancelLabel: 'Cancel'
    })
    if (!confirmed) return
    await closeCurrent()
  } catch (error) {
    notify.error('Could not close workbench', getErrorMessage(error))
  } finally {
    closingWorkbench = false
  }
}

export async function newWindow(): Promise<void> {
  await requireNative().window.create()
}

export function newEmptyFile(): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  createUntitledTextSurface(folderPath)
}

/** Native directory picker → open (or focus) that folder. */
export async function openFolderPicker(): Promise<void> {
  try {
    const path = await requireNative().dialogs.selectDirectory()
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
    await requireNative().settings.openGlobalFile()
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
    await backend.settings.openFolderFile(folderPath)
    await openTextFile(folderPath, '.sworm/settings.jsonc', { temporary: false })
  } catch (error) {
    notify.error('Open Folder Settings failed', getErrorMessage(error))
  }
}

export function revealActiveFolderInFileManager(): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath || splitRemotePath(folderPath)) return
  void requireNative()
    .files.reveal(folderPath)
    .catch((error) => {
      notify.error('Reveal in file manager failed', getErrorMessage(error))
    })
}

export function openActiveFolderInExternalTerminal(): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  void requireNative()
    .files.openInTerminal(folderPath)
    .catch((error) => {
      notify.error('Open in terminal failed', getErrorMessage(error))
    })
}

export function createSession(providerId: string, label: string): void {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return
  if (!getConnectedProviders(folderPath).some((p) => p.id === providerId)) {
    notify.error(`${label} unavailable`, `Connect the ${label} provider in Settings first.`)
    return
  }
  startSession(folderPath, providerId, `${label} session`)
}

export function newTerminalSession(): void {
  createSession('terminal', 'Terminal')
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
    notify.error('Cannot re-run task', 'The last task is no longer defined in .sworm/tasks.jsonc')
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
  void reopenLastClosedTab().catch((e) => notify.error('Reopen tab failed', getErrorMessage(e)))
}
