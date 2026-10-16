// Browser — one location palette for navigation or a host-pinned folder selection.
// Navigation captures the temporary tab, if any, that the chosen location replaces.

import { tick } from 'svelte'
import { backend } from '$lib/api/backend'
import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
import { openWorkbench } from '$lib/features/workbench/groups.svelte'
import type { TabId } from '$lib/features/workbench/model'
import { closeTab, getReplaceableTemporaryTab, openFolder } from '$lib/features/workbench/state.svelte'
import { platform } from '$lib/platform'
import type { WorkbenchInfo } from '$lib/types/backend'
import { closeTransientModals, registerModal } from '$lib/utils/modalRegistry.svelte'
import { splitRemotePath } from '$lib/utils/paths'

export interface NavigateBrowserRequest {
  kind?: 'navigate'
  /** Start browsing inside this folder. */
  path?: string
  /** Start inside this host's working directory; `null` is this host. Wins over `path`. */
  server?: string | null
  /** Captured preview that the chosen place replaces, if it is still replaceable. */
  replaceTabId?: TabId
}

export interface SelectFolderBrowserRequest {
  kind: 'select-folder'
  /** Start browsing here and keep selection on this path's host. */
  path: string
  server: string | null
}

export type BrowserRequest = NavigateBrowserRequest | SelectFolderBrowserRequest

let request = $state<BrowserRequest | null>(null)
let resolveSelection: ((path: string | null) => void) | null = null
let selectionFocus: HTMLElement | null = null

export function isBrowserOpen(): boolean {
  return request !== null
}

export function getBrowserRequest(): BrowserRequest | null {
  return request
}

export function openBrowser(options: NavigateBrowserRequest = {}): void {
  const replaceTabId = options.replaceTabId ?? getReplaceableTemporaryTab()?.id
  const wasOpen = isBrowserOpen()
  closeBrowser()
  if (!wasOpen) closeTransientModals()
  request = { ...options, replaceTabId }
}

/** Choose an existing folder without dismissing the dialog that requested it. */
export function selectFolder(options: { path: string }): Promise<string | null> {
  closeBrowser()
  selectionFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const { promise, resolve } = Promise.withResolvers<string | null>()
  resolveSelection = resolve
  request = {
    kind: 'select-folder',
    path: options.path,
    server: splitRemotePath(options.path)?.server ?? null
  }
  return promise
}

function finishSelection(path: string | null): void {
  const resolve = resolveSelection
  const focusTarget = selectionFocus
  resolveSelection = null
  selectionFocus = null
  request = null
  resolve?.(path)
  if (focusTarget) {
    // The awaiting dialog re-enables its fields after settlement, before focus returns.
    void tick().then(() => {
      if (!request && focusTarget.isConnected) focusTarget.focus({ preventScroll: true })
    })
  }
}

export function closeBrowser(): void {
  finishSelection(null)
}

/** All selection gestures canonicalize here; a replaced or cancelled request stays settled. */
export async function completeFolderSelection(path: string, opening: SelectFolderBrowserRequest): Promise<void> {
  if (request !== opening) return
  if ((splitRemotePath(path)?.server ?? null) !== opening.server) {
    throw new Error('Select a folder on the initiating host.')
  }
  const resolved = await backend.folders.resolve(path)
  if (request !== opening) return
  if ((splitRemotePath(resolved.path)?.server ?? null) !== opening.server) {
    throw new Error('Select a folder on the initiating host.')
  }
  finishSelection(resolved.path)
}

/** Status-bar folder chip: toggles browsing inside `path`. */
export function toggleBrowser(options: NavigateBrowserRequest = {}): void {
  if (request) closeBrowser()
  else openBrowser(options)
}

registerModal({ isOpen: isBrowserOpen, close: closeBrowser })

/** What the local host is called next to paired servers; read after the platform installs. */
export function localHostLabel(): string {
  return platform.native ? 'This Machine' : 'This Server'
}

/** Open the chosen folder without retaining the requesting preview. */
export async function goToFolder(path: string, replaceTabId?: TabId): Promise<void> {
  closeBrowser()
  await openFolder(path, replaceTabId)
}

/**
 * Join a workbench: a server's attaches into this window, the web page's
 * own host switches this page to it. A requesting preview closes after a join.
 */
export async function goToWorkbench(
  workbench: WorkbenchInfo,
  server?: string,
  takeover = false,
  replaceTabId?: TabId
): Promise<void> {
  closeBrowser()
  const joined = await runNotifiedTask(
    async () => {
      if (server) await openWorkbench(server, workbench.id, takeover)
      else if (platform.workbench.takeOver) await platform.workbench.takeOver(workbench.id)
      else throw new Error('Switching workbenches is not supported here')
      return true
    },
    {
      loading: { title: takeover ? 'Taking over workbench' : 'Opening workbench', description: server },
      error: { title: takeover ? 'Take over failed' : 'Open workbench failed' }
    }
  )
  // Web navigation replaces the document; cancellation must retain its tabs.
  if (joined && server && replaceTabId && getReplaceableTemporaryTab(replaceTabId)) closeTab(replaceTabId)
}
