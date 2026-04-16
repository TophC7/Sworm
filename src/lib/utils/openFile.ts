// Unified file-open logic.
//
// All text files open in the built-in Monaco editor tab.
// Fresh sessions remain available for agent CLIs but no longer
// participate in file opening.

import { backend } from '$lib/api/backend'
import { getAllTabs, addSessionTab, addEditorTab } from '$lib/stores/workspace.svelte'
import { createSession } from '$lib/stores/sessions.svelte'

/** Ensure a Fresh session tab exists for a project, creating one if needed. */
export async function ensureFreshSession(projectId: string): Promise<void> {
  const tabs = getAllTabs(projectId)
  const hasFresh = tabs.some((t) => t.kind === 'session' && t.providerId === 'fresh')
  if (hasFresh) return

  const session = await createSession(projectId, 'fresh', 'Fresh')
  addSessionTab(projectId, session.id, session.title, session.provider_id)
  await waitForSessionReady(session.id)
}

/** Poll until the session PTY transitions to 'running', with exponential backoff. */
async function waitForSessionReady(sessionId: string, timeoutMs = 8000): Promise<void> {
  const start = Date.now()
  let delay = 100

  while (Date.now() - start < timeoutMs) {
    await new Promise((r) => setTimeout(r, delay))
    try {
      const session = await backend.sessions.get(sessionId)
      if (session.status === 'running') return
      if (session.status === 'failed') throw new Error('Session failed to start')
    } catch {
      // Session may not be queryable yet — keep waiting
    }
    delay = Math.min(delay * 2, 1600)
  }
}

/** Open a file in the built-in editor tab. */
export function openFile(projectId: string, _projectPath: string, filePath: string): void {
  addEditorTab(projectId, filePath)
}
