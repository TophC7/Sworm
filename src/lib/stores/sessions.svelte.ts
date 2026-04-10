// Session state module using Svelte 5 runes.
//
// Tracks sessions for the active project and active session selection.

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

export function selectSession(id: string | null) {
	activeSessionId = id;
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
