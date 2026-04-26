// Session state module using Svelte 5 runes.
//
// Manages per-project session lists (CRUD against the backend).
// Active-session identity is derived from the workspace tab model.

import { backend } from '$lib/api/backend'
import { removeSession as removeActivityEntry } from '$lib/features/sessions/state/sessionActivity.svelte'
import { addSessionTab, restoreWorkspaceFromDisk, syncSessionTabs } from '$lib/features/workbench/state.svelte'
import type { Session } from '$lib/types/backend'

let sessionsByProject = $state<Map<string, Session[]>>(new Map())
let archivedSessionsByProject = $state<Map<string, Session[]>>(new Map())

function setProjectSessions(projectId: string, nextSessions: Session[]) {
  sessionsByProject = new Map(sessionsByProject).set(projectId, nextSessions)
}

function setProjectArchivedSessions(projectId: string, nextSessions: Session[]) {
  archivedSessionsByProject = new Map(archivedSessionsByProject).set(projectId, nextSessions)
}

export function getSessions(projectId: string): Session[] {
  return sessionsByProject.get(projectId) ?? []
}

export function getArchivedSessions(projectId: string): Session[] {
  return archivedSessionsByProject.get(projectId) ?? []
}

export function hasRunningSessions(projectId: string): boolean {
  return getSessions(projectId).some((s) => s.status === 'running')
}

// SELECTORS //

// Providers that aren't agent CLIs and shouldn't surface in history.
const NON_AGENT_PROVIDERS = new Set<string>(['terminal', 'fresh'])

function filterAgents(list: Session[]): Session[] {
  return list.filter((s) => !NON_AGENT_PROVIDERS.has(s.provider_id))
}

function groupByProvider(list: Session[]): Map<string, Session[]> {
  const groups = new Map<string, Session[]>()
  for (const session of list) {
    const existing = groups.get(session.provider_id)
    if (existing) existing.push(session)
    else groups.set(session.provider_id, [session])
  }
  return groups
}

// Component callers wrap these in `$derived`, so per-render memoization
// already handles repeat reads against the same underlying array.
export function getAgentSessions(projectId: string): Session[] {
  return filterAgents(getSessions(projectId))
}

export function getArchivedAgentSessions(projectId: string): Session[] {
  return filterAgents(getArchivedSessions(projectId))
}

export function getAgentSessionsGrouped(projectId: string): Map<string, Session[]> {
  return groupByProvider(getAgentSessions(projectId))
}

export function getArchivedAgentSessionsGrouped(projectId: string): Map<string, Session[]> {
  return groupByProvider(getArchivedAgentSessions(projectId))
}

// BACKEND CRUD //
export async function loadSessions(projectId: string) {
  try {
    // Hydrate the persisted workspace before reconciling; otherwise
    // the bootstrap heuristic in syncSessionTabs would race with the
    // restore and create duplicate session tabs. restoreWorkspaceFromDisk
    // is idempotent, so the eager-open path (already restored) just
    // returns immediately.
    await restoreWorkspaceFromDisk(projectId)
    const nextSessions = await backend.sessions.list(projectId)
    setProjectSessions(projectId, nextSessions)
    syncSessionTabs(projectId, nextSessions)
  } catch (e) {
    console.error('Failed to load sessions:', e)
    setProjectSessions(projectId, [])
  }
}

export async function loadArchivedSessions(projectId: string) {
  try {
    setProjectArchivedSessions(projectId, await backend.sessions.listArchived(projectId))
  } catch (e) {
    console.error('Failed to load archived sessions:', e)
    setProjectArchivedSessions(projectId, [])
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

export function updateSessionInList(projectId: string, sessionId: string, updates: Partial<Session>) {
  const projectSessions = sessionsByProject.get(projectId)
  if (!projectSessions || !projectSessions.some((session) => session.id === sessionId)) return
  setProjectSessions(
    projectId,
    projectSessions.map((session) => (session.id === sessionId ? { ...session, ...updates } : session))
  )
}

/** Create a session and open it as a tab in one step. */
export async function createAndOpenSession(projectId: string, providerId: string, title: string): Promise<Session> {
  const session = await createSession(projectId, providerId, title)
  addSessionTab(projectId, session.id, session.title, session.provider_id)
  return session
}
