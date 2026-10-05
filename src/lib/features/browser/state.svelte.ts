// Browser — one location palette for current/recent folders, workbenches,
// servers, and folder navigation. A request captures the temporary tab, if
// any, that the chosen location replaces.

import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
import { openWorkbench } from '$lib/features/workbench/groups.svelte'
import type { TabId } from '$lib/features/workbench/model'
import { closeTab, getReplaceableTemporaryTab, openFolder } from '$lib/features/workbench/state.svelte'
import { platform } from '$lib/platform'
import type { WorkbenchInfo } from '$lib/types/backend'
import { closeTransientModals, registerModal } from '$lib/utils/modalRegistry.svelte'

export interface BrowserRequest {
  /** Start browsing inside this folder. */
  path?: string
  /** Start inside this host's working directory; `null` is this host. Wins over `path`. */
  server?: string | null
  /** Captured preview that the chosen place replaces, if it is still replaceable. */
  replaceTabId?: TabId
}

let request = $state<BrowserRequest | null>(null)

export function isBrowserOpen(): boolean {
  return request !== null
}

export function getBrowserRequest(): BrowserRequest | null {
  return request
}

export function openBrowser(options: BrowserRequest = {}): void {
  const replaceTabId = options.replaceTabId ?? getReplaceableTemporaryTab()?.id
  if (!request) closeTransientModals()
  request = { ...options, replaceTabId }
}

export function closeBrowser(): void {
  request = null
}

/** Status-bar folder chip: toggles browsing inside `path`. */
export function toggleBrowser(options: BrowserRequest = {}): void {
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
