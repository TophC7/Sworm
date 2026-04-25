// Shared app actions.
//
// Button clicks, command-palette entries, and global shortcuts all call
// these functions so confirmation and side effects stay on one path.

import { backend } from '$lib/api/backend'
import { showProjectPicker } from '$lib/features/app-shell/project-picker/state.svelte'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage, runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
import { addProject, getActiveProject } from '$lib/features/projects/state.svelte'
import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
import { createAndOpenSession, hasRunningSessions } from '$lib/features/sessions/state/sessions.svelte'
import { setSettingsOpen } from '$lib/features/settings/dialog/state.svelte'
import { getLastTaskId, rerunLastTask } from '$lib/features/tasks/service.svelte'
import { openCommandPaletteWithSearch } from '$lib/features/command-palette/state.svelte'
import { ensureFreshSession } from '$lib/features/workbench/surfaces/session/service.svelte'
import {
  createUntitledTextSurface,
  getDirtyTextSurfaceCount,
  hasAnyDirtyTextSurfaces
} from '$lib/features/workbench/surfaces/text/service.svelte'
import {
  closeProject,
  flushPersistencePending,
  getActiveProjectId,
  openProject,
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

export async function openProjectDirectory(): Promise<void> {
  try {
    const path = await backend.projects.selectDirectory()
    if (!path) return
    const project = await addProject(path)
    openProject(project.id)
  } catch (error) {
    notify.error('Failed to add project', getErrorMessage(error))
  }
}

export function openSettings(): void {
  setSettingsOpen(true)
}

export function closeActiveProject(): void {
  const projectId = getActiveProjectId()
  if (!projectId) return
  void closeProject(projectId)
}

export function revealActiveProjectInFileManager(): void {
  const project = getActiveProject()
  if (!project) return
  void revealItemInDir(project.path).catch((error) => {
    notify.error('Reveal in file manager failed', getErrorMessage(error))
  })
}

export function openActiveProjectInExternalTerminal(): void {
  const project = getActiveProject()
  if (!project) return
  void backend.projects.openInTerminal(project.path).catch((error) => {
    notify.error('Open in terminal failed', getErrorMessage(error))
  })
}

export async function createSessionWithSharedWorkspaceWarning(providerId: string, label: string): Promise<boolean> {
  const projectId = getActiveProjectId()
  if (!projectId) return false
  const connected = getConnectedProviders()
  if (!connected.some((p) => p.id === providerId)) {
    notify.error(`${label} unavailable`, `Connect the ${label} provider in Settings first.`)
    return false
  }

  if (hasRunningSessions(projectId)) {
    const proceed = await confirmAsync({
      title: 'Shared Workspace Warning',
      message:
        'Another session is already running in this project.\n\n' +
        'Sessions in the same project share the same working tree and branch.\n' +
        'Changes made by one session may conflict with another.',
      confirmLabel: 'Start Anyway',
      cancelLabel: 'Cancel'
    })
    if (!proceed) return false
  }

  const created = await runNotifiedTask(() => createAndOpenSession(projectId, providerId, `${label} session`), {
    loading: { title: `Starting ${label} session` },
    error: { title: `Failed to start ${label} session` }
  })
  return !!created
}

export async function newTerminalSession(): Promise<void> {
  await createSessionWithSharedWorkspaceWarning('terminal', 'Terminal')
}

export async function openFreshSession(): Promise<void> {
  const projectId = getActiveProjectId()
  if (!projectId) return
  if (!getConnectedProviders().some((provider) => provider.id === 'fresh')) {
    notify.error('Fresh unavailable', 'Connect the Fresh provider in Settings first.')
    return
  }
  await runNotifiedTask(() => ensureFreshSession(projectId), {
    loading: { title: 'Opening Fresh' },
    error: { title: 'Failed to open Fresh' }
  })
}

export function showTasks(): void {
  openCommandPaletteWithSearch('! ')
}

export async function rerunLastProjectTask(): Promise<void> {
  const projectId = getActiveProjectId()
  if (!projectId) return
  const lastId = getLastTaskId(projectId)
  if (!lastId) return
  const tabId = await rerunLastTask(projectId)
  if (tabId === null) {
    notify.error('Cannot re-run task', 'The last task is no longer defined in .sworm/tasks.json')
  }
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
