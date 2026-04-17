// Session state module using Svelte 5 runes.
//
// Manages the session list for the active project (CRUD against the backend).
// Active-session identity is derived from the workspace tab model.

import { backend } from '$lib/api/backend'
import { removeSession as removeActivityEntry } from '$lib/stores/activity.svelte'
import { addSessionTab, restoreWorkspaceFromDisk, syncSessionTabs } from '$lib/stores/workspace.svelte'
import type { Session } from '$lib/types/backend'

let sessions = $state<Session[]>([])
let archivedSessions = $state<Session[]>([])

export function getSessions() {
  return sessions
}

export function getArchivedSessions() {
  return archivedSessions
}

export function hasRunningSessions(): boolean {
  return sessions.some((s) => s.status === 'running')
}

// --- Backend CRUD ---

export async function loadSessions(projectId: string) {
  try {
    // Hydrate the persisted workspace before reconciling — otherwise
    // the bootstrap heuristic in syncSessionTabs would race with the
    // restore and create duplicate session tabs. restoreWorkspaceFromDisk
    // is idempotent, so the eager-open path (already restored) just
    // returns immediately.
    await restoreWorkspaceFromDisk(projectId)
    sessions = await backend.sessions.list(projectId)
    syncSessionTabs(projectId, sessions)
  } catch (e) {
    console.error('Failed to load sessions:', e)
    sessions = []
  }
}

export async function loadArchivedSessions(projectId: string) {
  try {
    archivedSessions = await backend.sessions.listArchived(projectId)
  } catch (e) {
    console.error('Failed to load archived sessions:', e)
    archivedSessions = []
  }
}

export async function createSession(projectId: string, providerId: string, title: string): Promise<Session> {
  const session = await backend.sessions.create(projectId, providerId, title)
  await loadSessions(projectId)
  return session
}

export async function removeSession(sessionId: string, projectId: string) {
  removeActivityEntry(sessionId)
  await backend.sessions.remove(sessionId)
  await loadSessions(projectId)
  await loadArchivedSessions(projectId)
}

export async function archiveSession(sessionId: string, projectId: string) {
  await backend.sessions.archive(sessionId)
  await loadSessions(projectId)
  await loadArchivedSessions(projectId)
}

export async function unarchiveSession(sessionId: string, projectId: string) {
  await backend.sessions.unarchive(sessionId)
  await loadSessions(projectId)
  await loadArchivedSessions(projectId)
}

export function updateSessionInList(sessionId: string, updates: Partial<Session>) {
  sessions = sessions.map((s) => (s.id === sessionId ? { ...s, ...updates } : s))
}

/** Create a session and open it as a tab in one step. */
export async function createAndOpenSession(projectId: string, providerId: string, title: string): Promise<Session> {
  const session = await createSession(projectId, providerId, title)
  addSessionTab(projectId, session.id, session.title, session.provider_id)
  return session
}
