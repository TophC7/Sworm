// Unified file-open logic.
//
// Routes files to the appropriate handler: built-in viewers for
// supported types (markdown), Fresh editor for everything else.

import { backend } from '$lib/api/backend'
import { getAllTabs, addSessionTab, addMarkdownTab } from '$lib/stores/workspace.svelte'
import { createSession } from '$lib/stores/sessions.svelte'

const BUILTIN_EXTENSIONS = ['.md', '.mdx']

function isBuiltinFile(filePath: string): boolean {
  return BUILTIN_EXTENSIONS.some((ext) => filePath.endsWith(ext))
}

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

/**
 * Open a file from the project — routes to the built-in viewer
 * for supported types, or to the Fresh editor for everything else.
 */
export async function openFile(projectId: string, projectPath: string, filePath: string): Promise<void> {
  if (isBuiltinFile(filePath)) {
    addMarkdownTab(projectId, filePath)
    return
  }

  await ensureFreshSession(projectId)
  await backend.editor.openFile(projectId, projectPath, filePath)
}
