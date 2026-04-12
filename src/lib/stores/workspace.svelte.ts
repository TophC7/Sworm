// Workspace state module — central tab + pane layout state.
//
// Manages which projects are open as tabs, which session/diff tabs exist
// within each project, and which pane each tab is assigned to.
// Replaces activeProjectId (from projects store) and activeSessionId
// (from sessions store) with a richer per-pane model.

import type { DiffContext } from '$lib/types/backend';
import * as sessionRegistry from '$lib/terminal/sessionRegistry';
import { clearGitState } from '$lib/stores/git.svelte';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type TabId = string;

export type PaneSlot =
	| 'sole'
	| 'left'
	| 'right'
	| 'top-left'
	| 'top-right'
	| 'bottom-left'
	| 'bottom-right';

export interface SessionTab {
	kind: 'session';
	id: TabId;
	sessionId: string;
	title: string;
	providerId: string;
}

export interface DiffTab {
	kind: 'diff';
	id: TabId;
	filePath: string;
	context: DiffContext;
	temporary: boolean;
}

export type Tab = SessionTab | DiffTab;

export interface PaneState {
	slot: PaneSlot;
	tabs: TabId[];
	activeTabId: TabId | null;
}

export type SplitMode = 'single' | 'horizontal' | 'vertical' | 'quad';

export interface ProjectWorkspace {
	projectId: string;
	tabs: Tab[];
	panes: PaneState[];
	splitMode: SplitMode;
}

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let openProjectIds = $state<string[]>([]);
let activeProjectId = $state<string | null>(null);
let workspaces = $state<Map<string, ProjectWorkspace>>(new Map());
let focusedPaneSlot = $state<PaneSlot>('sole');

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

let nextTabId = 0;
function generateTabId(): TabId {
	return `tab-${Date.now()}-${nextTabId++}`;
}

function defaultPane(): PaneState {
	return { slot: 'sole', tabs: [], activeTabId: null };
}

function getWorkspace(projectId: string): ProjectWorkspace | undefined {
	return workspaces.get(projectId);
}

function ensureWorkspace(projectId: string): ProjectWorkspace {
	let ws = workspaces.get(projectId);
	if (!ws) {
		ws = {
			projectId,
			tabs: [],
			panes: [defaultPane()],
			splitMode: 'single'
		};
		workspaces.set(projectId, ws);
	}
	return ws;
}

/** Find which pane a tab lives in. */
function findPaneForTab(ws: ProjectWorkspace, tabId: TabId): PaneState | undefined {
	return ws.panes.find((p) => p.tabs.includes(tabId));
}

/** Get the pane the user is focused on, or the first pane. */
function activePaneOf(ws: ProjectWorkspace): PaneState {
	return ws.panes.find((p) => p.slot === focusedPaneSlot) ?? ws.panes[0];
}

// ---------------------------------------------------------------------------
// Project tab operations
// ---------------------------------------------------------------------------

export function getOpenProjectIds(): string[] {
	return openProjectIds;
}

export function getActiveProjectId(): string | null {
	return activeProjectId;
}

export function openProject(projectId: string) {
	if (!openProjectIds.includes(projectId)) {
		openProjectIds = [...openProjectIds, projectId];
	}
	ensureWorkspace(projectId);
	activeProjectId = projectId;
}

export function closeProject(projectId: string) {
	// Dispose all session PTYs for this project
	const ws = getWorkspace(projectId);
	if (ws) {
		for (const tab of ws.tabs) {
			if (tab.kind === 'session') {
				sessionRegistry.dispose(tab.sessionId);
			}
		}
		workspaces.delete(projectId);
	}

	// Stop git polling and clear cached summary
	clearGitState(projectId);

	openProjectIds = openProjectIds.filter((id) => id !== projectId);

	if (activeProjectId === projectId) {
		activeProjectId = openProjectIds.length > 0 ? openProjectIds[openProjectIds.length - 1] : null;
	}
}

export function selectProject(projectId: string | null) {
	if (projectId && openProjectIds.includes(projectId)) {
		activeProjectId = projectId;
	} else if (projectId === null) {
		activeProjectId = null;
	}
}

export function reorderProjects(fromIndex: number, toIndex: number) {
	if (
		fromIndex < 0 ||
		toIndex < 0 ||
		fromIndex >= openProjectIds.length ||
		toIndex >= openProjectIds.length
	) {
		return;
	}
	const next = [...openProjectIds];
	const [moved] = next.splice(fromIndex, 1);
	next.splice(toIndex, 0, moved);
	openProjectIds = next;
}

// ---------------------------------------------------------------------------
// Tab operations
// ---------------------------------------------------------------------------

export function getWorkspaceForProject(projectId: string): ProjectWorkspace | undefined {
	return getWorkspace(projectId);
}

export function getActiveWorkspace(): ProjectWorkspace | undefined {
	return activeProjectId ? getWorkspace(activeProjectId) : undefined;
}

export function addSessionTab(projectId: string, sessionId: string, title: string, providerId: string): TabId {
	const ws = ensureWorkspace(projectId);
	// Check if a tab for this session already exists
	const existing = ws.tabs.find((t) => t.kind === 'session' && t.sessionId === sessionId);
	if (existing) {
		// Activate it instead of creating a duplicate
		const pane = findPaneForTab(ws, existing.id) ?? activePaneOf(ws);
		pane.activeTabId = existing.id;
		return existing.id;
	}

	const tab: SessionTab = {
		kind: 'session',
		id: generateTabId(),
		sessionId,
		title,
		providerId
	};

	ws.tabs = [...ws.tabs, tab];
	const pane = activePaneOf(ws);
	pane.tabs = [...pane.tabs, tab.id];
	pane.activeTabId = tab.id;

	return tab.id;
}

export function addDiffTab(
	projectId: string,
	filePath: string,
	context: DiffContext,
	temporary: boolean
): TabId {
	const ws = ensureWorkspace(projectId);
	const pane = activePaneOf(ws);

	// If temporary, replace existing temporary diff tab in this pane
	if (temporary) {
		const existingTempId = pane.tabs.find((id) => {
			const tab = ws.tabs.find((t) => t.id === id);
			return tab?.kind === 'diff' && tab.temporary;
		});

		if (existingTempId) {
			// Update the existing temporary tab in place
			ws.tabs = ws.tabs.map((t) =>
				t.id === existingTempId
					? { kind: 'diff' as const, id: existingTempId, filePath, context, temporary: true }
					: t
			);
			pane.activeTabId = existingTempId;
			return existingTempId;
		}
	}

	// Check if a persistent tab for this file already exists
	const existing = ws.tabs.find(
		(t) => t.kind === 'diff' && t.filePath === filePath && !t.temporary
	);
	if (existing) {
		const existingPane = findPaneForTab(ws, existing.id) ?? pane;
		existingPane.activeTabId = existing.id;
		return existing.id;
	}

	const tab: DiffTab = {
		kind: 'diff',
		id: generateTabId(),
		filePath,
		context,
		temporary
	};

	ws.tabs = [...ws.tabs, tab];
	pane.tabs = [...pane.tabs, tab.id];
	pane.activeTabId = tab.id;

	return tab.id;
}

export function promoteTemporaryTab(tabId: TabId) {
	const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined;
	if (!ws) return;

	ws.tabs = ws.tabs.map((t) =>
		t.id === tabId && t.kind === 'diff' && t.temporary ? { ...t, temporary: false } : t
	);
}

export function closeTab(projectId: string, tabId: TabId) {
	const ws = getWorkspace(projectId);
	if (!ws) return;

	const tab = ws.tabs.find((t) => t.id === tabId);
	if (!tab) return;

	// Kill PTY if session tab
	if (tab.kind === 'session') {
		sessionRegistry.dispose(tab.sessionId);
	}

	// Remove from pane
	for (const pane of ws.panes) {
		if (pane.tabs.includes(tabId)) {
			pane.tabs = pane.tabs.filter((id) => id !== tabId);
			if (pane.activeTabId === tabId) {
				pane.activeTabId = pane.tabs.length > 0 ? pane.tabs[pane.tabs.length - 1] : null;
			}
			break;
		}
	}

	// Remove from workspace tabs
	ws.tabs = ws.tabs.filter((t) => t.id !== tabId);
}

export function setActiveTab(projectId: string, paneSlot: PaneSlot, tabId: TabId) {
	const ws = getWorkspace(projectId);
	if (!ws) return;

	const pane = ws.panes.find((p) => p.slot === paneSlot);
	if (pane && pane.tabs.includes(tabId)) {
		pane.activeTabId = tabId;
	}
}

export function getTabById(projectId: string, tabId: TabId): Tab | undefined {
	return getWorkspace(projectId)?.tabs.find((t) => t.id === tabId);
}

export function getAllTabs(projectId: string): Tab[] {
	return getWorkspace(projectId)?.tabs ?? [];
}

// ---------------------------------------------------------------------------
// Pane focus
// ---------------------------------------------------------------------------

export function getFocusedPaneSlot(): PaneSlot {
	return focusedPaneSlot;
}

export function setFocusedPane(slot: PaneSlot) {
	focusedPaneSlot = slot;
}

// ---------------------------------------------------------------------------
// Split pane operations
// ---------------------------------------------------------------------------

/**
 * Split the current focused pane in a direction.
 * Returns the slot of the new pane, or null if already at max.
 *
 * single → horizontal (Split Right) or vertical (Split Down)
 * horizontal/vertical → quad
 * quad → null (max 4 panes)
 */
export function splitPane(
	projectId: string,
	direction: 'right' | 'down'
): PaneSlot | null {
	const ws = getWorkspace(projectId);
	if (!ws) return null;

	const currentMode = ws.splitMode;
	let newMode: SplitMode;
	let newSlot: PaneSlot;

	// Remember which pane the user is focused on BEFORE we reassign slots
	const focusedBefore = focusedPaneSlot;

	if (currentMode === 'single') {
		if (direction === 'right') {
			newMode = 'horizontal';
			ws.panes[0].slot = 'left';
			newSlot = 'right';
		} else {
			newMode = 'vertical';
			ws.panes[0].slot = 'left'; // top pane
			newSlot = 'right'; // bottom pane
		}
	} else if ((currentMode === 'horizontal' || currentMode === 'vertical') && ws.panes.length === 2) {
		newMode = 'quad';
		const [pane0, pane1] = ws.panes;
		const was1Focused = focusedBefore === pane1.slot;

		// Reassign existing panes to top row
		pane0.slot = 'top-left';
		pane1.slot = 'top-right';

		if (direction === 'right') {
			// New pane appears to the right of the focused pane
			newSlot = was1Focused ? 'bottom-right' : 'bottom-left';
		} else {
			// New pane appears below the focused pane
			newSlot = was1Focused ? 'bottom-right' : 'bottom-left';
		}
	} else if (currentMode === 'quad') {
		return null;
	} else {
		return null;
	}

	const newPane: PaneState = { slot: newSlot, tabs: [], activeTabId: null };
	ws.panes = [...ws.panes, newPane];
	ws.splitMode = newMode;
	focusedPaneSlot = newSlot;

	return newSlot;
}

/**
 * Move a tab from its current pane to a target pane slot.
 * If the target pane doesn't exist, creates it via split.
 */
export function moveTabToPane(projectId: string, tabId: TabId, targetSlot: PaneSlot) {
	const ws = getWorkspace(projectId);
	if (!ws) return;

	// Remove from current pane
	for (const pane of ws.panes) {
		if (pane.tabs.includes(tabId)) {
			pane.tabs = pane.tabs.filter((id) => id !== tabId);
			if (pane.activeTabId === tabId) {
				pane.activeTabId = pane.tabs.length > 0 ? pane.tabs[pane.tabs.length - 1] : null;
			}
			break;
		}
	}

	// Add to target pane
	let targetPane = ws.panes.find((p) => p.slot === targetSlot);
	if (!targetPane) {
		targetPane = { slot: targetSlot, tabs: [], activeTabId: null };
		ws.panes = [...ws.panes, targetPane];
	}
	targetPane.tabs = [...targetPane.tabs, tabId];
	targetPane.activeTabId = tabId;
}

/**
 * Collapse empty panes and simplify split mode.
 * Called after a tab is closed or moved.
 */
export function collapsePaneIfEmpty(projectId: string) {
	const ws = getWorkspace(projectId);
	if (!ws) return;

	// Remove empty panes (keep at least one)
	const nonEmpty = ws.panes.filter((p) => p.tabs.length > 0);
	if (nonEmpty.length === 0) {
		// All panes empty — reset to single with one empty pane
		ws.panes = [defaultPane()];
		ws.splitMode = 'single';
		focusedPaneSlot = 'sole';
		return;
	}

	ws.panes = nonEmpty;

	// Simplify mode based on remaining pane count
	if (nonEmpty.length === 1) {
		nonEmpty[0].slot = 'sole';
		ws.splitMode = 'single';
		focusedPaneSlot = 'sole';
	} else if (nonEmpty.length === 2 && ws.splitMode === 'quad') {
		// Downgrade from quad to horizontal
		nonEmpty[0].slot = 'left';
		nonEmpty[1].slot = 'right';
		ws.splitMode = 'horizontal';
		if (focusedPaneSlot !== 'left' && focusedPaneSlot !== 'right') {
			focusedPaneSlot = 'left';
		}
	}
	// 3 panes in quad mode — leave as quad with one empty slot
}

export function getPanes(projectId: string): PaneState[] {
	return getWorkspace(projectId)?.panes ?? [];
}

export function getSplitMode(projectId: string): SplitMode {
	return getWorkspace(projectId)?.splitMode ?? 'single';
}

// ---------------------------------------------------------------------------
// Compatibility layer
//
// These re-export the active project/session concept so existing components
// continue to work during the incremental migration.
// ---------------------------------------------------------------------------

/** The active session is the session tab that's active in the focused pane. */
export function getActiveSessionId(): string | null {
	const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined;
	if (!ws) return null;

	const pane = ws.panes.find((p) => p.slot === focusedPaneSlot) ?? ws.panes[0];
	if (!pane?.activeTabId) return null;

	const tab = ws.tabs.find((t) => t.id === pane.activeTabId);
	return tab?.kind === 'session' ? tab.sessionId : null;
}
