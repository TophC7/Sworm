import { TerminalSessionManager } from '$lib/terminal/TerminalSessionManager';

const sessions = new Map<string, TerminalSessionManager>();

export function attach(sessionId: string, container: HTMLElement): TerminalSessionManager {
	let manager = sessions.get(sessionId);
	if (!manager) {
		manager = new TerminalSessionManager(sessionId);
		sessions.set(sessionId, manager);
	}

	manager.attach(container);
	return manager;
}

export function detach(sessionId: string): void {
	sessions.get(sessionId)?.detach();
}

export function dispose(sessionId: string): void {
	const manager = sessions.get(sessionId);
	if (!manager) {
		return;
	}

	manager.dispose();
	sessions.delete(sessionId);
}

export function disposeAll(): void {
	for (const [sessionId, manager] of sessions) {
		manager.dispose();
		sessions.delete(sessionId);
	}
}

export function get(sessionId: string): TerminalSessionManager | undefined {
	return sessions.get(sessionId);
}
