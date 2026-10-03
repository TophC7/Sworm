// Browser — the one surface for going somewhere else: a places list
// (workbenches, recent folders, servers) and folder columns that browse any
// host. A request names where browsing starts and which new tab page, if
// any, the chosen place turns into.

import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
import { openWorkbench } from '$lib/features/workbench/groups.svelte'
import type { TabId } from '$lib/features/workbench/model'
import { closeTab, openFolder, retargetNewTab } from '$lib/features/workbench/state.svelte'
import { platform } from '$lib/platform'
import type { WorkbenchInfo } from '$lib/types/backend'
import { closeTransientModals, registerModal } from '$lib/utils/modalRegistry.svelte'

export interface BrowserRequest {
  /** Start in the folder columns with this folder selected in its parent. */
  path?: string
  /** Start inside this host's home folder; `null` is this host. Wins over `path`. */
  server?: string | null
  /** New tab page that becomes the chosen place instead of staying behind. */
  newTabId?: TabId
}

let request = $state<BrowserRequest | null>(null)

export function isBrowserOpen(): boolean {
  return request !== null
}

export function getBrowserRequest(): BrowserRequest | null {
  return request
}

export function openBrowser(options: BrowserRequest = {}): void {
  if (!request) closeTransientModals()
  request = options
}

export function closeBrowser(): void {
  request = null
}

/** Status-bar folder chip: toggles the columns at `path`. */
export function toggleBrowser(options: BrowserRequest = {}): void {
  if (request) closeBrowser()
  else openBrowser(options)
}

registerModal({ isOpen: isBrowserOpen, close: closeBrowser })

/** What the local host is called next to paired servers; read after the platform installs. */
export function localHostLabel(): string {
  return platform.native ? 'This Machine' : 'This Server'
}

/**
 * Browse a host from inside its home folder; `server` null is this host. An
 * unreachable server opens at its root so the listing shows why.
 */
export function browseServer(server: string | null, newTabId?: TabId): void {
  openBrowser({ server, newTabId })
}

/** Open a folder, turning `newTabId` into it when the request came from a new tab page. */
export async function goToFolder(path: string, newTabId?: TabId): Promise<void> {
  closeBrowser()
  if (newTabId) await retargetNewTab(newTabId, path)
  else await openFolder(path)
}

/**
 * Join a workbench: a server's attaches into this window, the web page's
 * own host switches this page to it. The new tab page that asked for it closes.
 */
export async function goToWorkbench(
  workbench: WorkbenchInfo,
  server?: string,
  takeover = false,
  newTabId?: TabId
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
  if (joined && newTabId) closeTab(newTabId)
}
