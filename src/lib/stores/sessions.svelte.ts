// Session state module using Svelte 5 runes.
//
// Manages the session list for the active project (CRUD against the backend).
// During Phase 0, activeSessionId still lives here for backward compatibility.
// Phase 1 will remove it and switch to workspace.svelte.ts tab model.

import { backend } from '$lib/api/backend';
import type { Session } from '$lib/types/backend';

let sessions = $state<Session[]>([]);
let activeSessionId = $state<string | null>(null);

let activeSession = $derived(sessions.find((s) => s.id === activeSessionId) ?? null);

export function getSessions() {
	return sessions;
}

export function getActiveSession() {
	return activeSession;
}

export function getActiveSessionId() {
	return activeSessionId;
}

export function hasRunningSessions(): boolean {
	return sessions.some((s) => s.status === 'running');
}

export function selectSession(id: string | null) {
	activeSessionId = id;
}

// --- Backend CRUD ---

export async function loadSessions(projectId: string) {
	try {
		sessions = await backend.sessions.list(projectId);
	} catch (e) {
		console.error('Failed to load sessions:', e);
		sessions = [];
	}
}

export function clearSessions() {
	sessions = [];
	activeSessionId = null;
}

export async function createSession(
	projectId: string,
	providerId: string,
	title: string
): Promise<Session> {
	const session = await backend.sessions.create(projectId, providerId, title);
	await loadSessions(projectId);
	return session;
}

export async function removeSession(sessionId: string, projectId: string) {
	await backend.sessions.remove(sessionId);
	if (activeSessionId === sessionId) {
		activeSessionId = null;
	}
	await loadSessions(projectId);
}

export function updateSessionInList(sessionId: string, updates: Partial<Session>) {
	sessions = sessions.map((s) => (s.id === sessionId ? { ...s, ...updates } : s));
}
