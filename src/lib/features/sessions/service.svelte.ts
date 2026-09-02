import { notify } from '$lib/features/notifications/state.svelte'
import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
import type { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'
import type { SessionTab, TabId } from '$lib/features/workbench/model'
import {
  addSessionTab,
  clearSessionTabResumeToken,
  setSessionTabResumeToken,
  setSessionTabStatus
} from '$lib/features/workbench/state.svelte'

/**
 * Open a dormant session tab for `folderPath`. The mounted
 * SessionTerminal spawns the process; nothing is persisted beyond the
 * tab itself.
 */
export function startSession(folderPath: string, providerId: string, title: string): TabId {
  return addSessionTab(folderPath, { providerId, title, resumeToken: null })
}

/**
 * Spawn the process behind a session tab. Snapshot the resume token
 * *before* spawning: the backend may bind a token during start, so
 * reading `tab.resumeToken` afterwards can't tell "resumed" from "just
 * bound". `clearSessionTabResumeToken` no-ops when discovery has
 * meanwhile set a different token.
 */
export async function startSessionProcess(manager: TerminalSessionManager, tab: SessionTab): Promise<void> {
  const supplied = tab.resumeToken
  setSessionTabStatus(tab.id, 'starting')
  const info = await manager.startPty({
    folderPath: tab.folderPath,
    providerId: tab.providerId,
    resumeToken: supplied
  })
  if (info.resumeToken) {
    setSessionTabResumeToken(tab.id, info.resumeToken)
  } else if (supplied !== null) {
    clearSessionTabResumeToken(tab.id, supplied)
    notify.info('Started a new conversation', 'The previous one no longer exists.')
  }
}

/** Stop a session tab's process. No manager means no run: nothing to stop. */
export async function stopSessionProcess(tabId: TabId): Promise<void> {
  await sessionRegistry.get(tabId)?.stopPty()
}
