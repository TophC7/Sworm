import { backend } from '$lib/api/backend'
import { notify } from '$lib/features/notifications/state.svelte'
import { providerLabel } from '$lib/features/sessions/providers/labels'
import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
import type { TerminalSessionManager } from '$lib/features/sessions/terminal/TerminalSessionManager'
import type { SessionTab, TabId } from '$lib/features/workbench/model'
import { addSessionTab, clearSessionTabResumeToken, setSessionTabStatus } from '$lib/features/workbench/state.svelte'

/**
 * Open a dormant session tab for `folderPath`. The mounted
 * SessionTerminal spawns the process; nothing is persisted beyond the
 * tab itself.
 */
export function startSession(folderPath: string, providerId: string, title: string): TabId {
  return addSessionTab(folderPath, {
    sessionId: crypto.randomUUID(),
    providerId,
    title,
    resumeToken: null
  })
}

/**
 * Spawn the process behind a session tab. Snapshot the resume token
 * *before* spawning: the backend binds a token during start (Claude
 * always, Codex/Antigravity when a new thread appears), so reading
 * `tab.resumeToken` afterwards can't tell "resumed" from "just bound".
 */
export async function startSessionProcess(manager: TerminalSessionManager, tab: SessionTab): Promise<void> {
  const { sessionId, folderPath, providerId, resumeToken } = tab
  setSessionTabStatus(sessionId, 'starting')
  const info = await manager.startPty({ sessionId, folderPath, providerId, resumeToken })
  if (!info.resumed && resumeToken !== null) {
    // Claude reuses its deterministic token for the fresh replacement.
    // Codex/Antigravity bind a different token asynchronously.
    if (providerId !== 'claude_code') clearSessionTabResumeToken(sessionId, resumeToken)
    notify.info('Previous conversation not found', `Started a new ${providerLabel(providerId)} conversation.`)
  }
}

/** Stop a session's process; falls back to the backend when no terminal manager is mounted. */
export async function stopSessionProcess(sessionId: string): Promise<void> {
  const manager = sessionRegistry.get(sessionId)
  if (manager) await manager.stopPty()
  else await backend.sessions.stop(sessionId)
}
