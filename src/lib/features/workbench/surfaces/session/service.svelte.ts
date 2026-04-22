import { backend } from '$lib/api/backend'
import { allProviders, directOptions } from '$lib/features/sessions/providers/catalog'
import { createSession, getSessions } from '$lib/features/sessions/state/sessions.svelte'
import type { Session } from '$lib/types/backend'
import type { SessionTab, TabId } from '$lib/features/workbench/model'
import { addSessionTab, openProject, restoreWorkspaceFromDisk } from '$lib/features/workbench/state.svelte'

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

async function waitForSessionReady(sessionId: string, timeoutMs = 8000): Promise<void> {
  const start = Date.now()
  let delay = 100

  while (Date.now() - start < timeoutMs) {
    await wait(delay)
    try {
      const session = await backend.sessions.get(sessionId)
      if (session.status === 'running') return
      if (session.status === 'failed') throw new Error('Session failed to start')
    } catch {
      // Session may not be queryable yet; keep polling until timeout.
    }
    delay = Math.min(delay * 2, 1600)
  }
}

function findProjectSession(projectId: string, providerId: string): Session | null {
  return getSessions().find((session) => session.project_id === projectId && session.provider_id === providerId) ?? null
}

async function resolveFreshSession(projectId: string): Promise<Session> {
  const existing = findProjectSession(projectId, 'fresh')
  if (existing) return existing
  return createSession(projectId, 'fresh', 'Fresh')
}

export async function ensureSessionSurface(
  projectId: string,
  sessionId: string,
  title: string,
  providerId: string
): Promise<TabId> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
  return addSessionTab(projectId, sessionId, title, providerId)
}

export async function ensureFreshSession(projectId: string): Promise<TabId> {
  // Fresh is a singleton session surface per project. Callers should
  // never create a second one; they resolve the existing session row
  // and reveal its tab instead.
  const session = await resolveFreshSession(projectId)
  const tabId = await ensureSessionSurface(projectId, session.id, session.title, session.provider_id)

  if (session.status === 'idle' || session.status === 'starting') {
    await waitForSessionReady(session.id)
  }

  return tabId
}

export function getSessionTabTitle(tab: SessionTab): string {
  return tab.title
}

export function getSessionProviderIcon(tab: SessionTab): string | null {
  const provider =
    allProviders.find((entry) => entry.id === tab.providerId) ??
    directOptions.find((entry) => entry.id === tab.providerId)
  return provider?.icon ?? null
}
