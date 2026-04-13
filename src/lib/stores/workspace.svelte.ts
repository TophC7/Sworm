// Workspace state module — central tab + pane layout state.
//
// Manages which projects are open as tabs, which session/diff tabs exist
// within each project, and which pane each tab is assigned to.
// Replaces activeProjectId (from projects store) and activeSessionId
// (from sessions store) with a richer per-pane model.

import type { DiffContext, Session } from '$lib/types/backend'
import * as sessionRegistry from '$lib/terminal/sessionRegistry'
import { clearGitState } from '$lib/stores/git.svelte'

// Statuses that warrant auto-creating a tab when syncing sessions.
// Historical sessions (stopped/exited/failed) stay in the DB for the
// session history view but don't reappear as tabs automatically.
const ACTIVE_STATUSES = new Set(['idle', 'starting', 'running'])

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type TabId = string

export type PaneSlot = 'sole' | 'left' | 'right' | 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right'

export interface SessionTab {
  kind: 'session'
  id: TabId
  sessionId: string
  title: string
  providerId: string
  locked: boolean
}

export interface DiffTab {
  kind: 'diff'
  id: TabId
  filePath: string
  context: DiffContext
  temporary: boolean
  locked: boolean
}

export type Tab = SessionTab | DiffTab

export interface PaneState {
  slot: PaneSlot
  tabs: TabId[]
  activeTabId: TabId | null
}

export type SplitMode = 'single' | 'horizontal' | 'vertical' | 'quad'
export type QuadLayout = 'top' | 'bottom' | 'left' | 'right' | null

export interface ProjectWorkspace {
  projectId: string
  tabs: Tab[]
  panes: PaneState[]
  splitMode: SplitMode
  quadLayout: QuadLayout
}

export interface DraggedTab {
  projectId: string
  tabId: TabId
}

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let openProjectIds = $state<string[]>([])
let activeProjectId = $state<string | null>(null)
let workspaces = $state<Map<string, ProjectWorkspace>>(new Map())
let focusedPaneSlot = $state<PaneSlot>('sole')
let draggedTab = $state<DraggedTab | null>(null)

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

let nextTabId = 0
function generateTabId(): TabId {
  return `tab-${Date.now()}-${nextTabId++}`
}

export function createPane(slot: PaneSlot): PaneState {
  return { slot, tabs: [], activeTabId: null }
}

function clonePane(pane: PaneState): PaneState {
  return {
    ...pane,
    tabs: [...pane.tabs]
  }
}

function getWorkspace(projectId: string): ProjectWorkspace | undefined {
  return workspaces.get(projectId)
}

/**
 * Trigger Svelte reactivity after mutating a workspace in place.
 *
 * Creates a shallow clone and replaces the Map entry so Svelte
 * sees a new reference. Call this once at the end of any function
 * that mutates workspace state.
 */
function commitWorkspace(ws: ProjectWorkspace) {
  workspaces = new Map(workspaces).set(ws.projectId, {
    ...ws,
    tabs: [...ws.tabs],
    panes: ws.panes.map(clonePane)
  })
}

function ensureWorkspace(projectId: string): ProjectWorkspace {
  let ws = workspaces.get(projectId)
  if (!ws) {
    ws = {
      projectId,
      tabs: [],
      panes: [createPane('sole')],
      splitMode: 'single',
      quadLayout: null
    }
    commitWorkspace(ws)
    // Re-read the committed (cloned) version from the map.
    ws = workspaces.get(projectId)!
  }
  return ws
}

/** Find which pane a tab lives in. */
function findPaneForTab(ws: ProjectWorkspace, tabId: TabId): PaneState | undefined {
  return ws.panes.find((p) => p.tabs.includes(tabId))
}

/** Get the pane the user is focused on, or the first pane. */
function activePaneOf(ws: ProjectWorkspace): PaneState {
  return ws.panes.find((p) => p.slot === focusedPaneSlot) ?? ws.panes[0]
}

function ensureActiveTab(pane: PaneState) {
  if (pane.activeTabId && pane.tabs.includes(pane.activeTabId)) {
    return
  }
  pane.activeTabId = pane.tabs[pane.tabs.length - 1] ?? null
}

function setPaneTabs(pane: PaneState, tabs: TabId[]) {
  pane.tabs = tabs
  ensureActiveTab(pane)
}

function removeTabFromPane(pane: PaneState, tabId: TabId) {
  if (!pane.tabs.includes(tabId)) return
  setPaneTabs(
    pane,
    pane.tabs.filter((id) => id !== tabId)
  )
}

function findTab(ws: ProjectWorkspace, tabId: TabId): Tab | undefined {
  return ws.tabs.find((t) => t.id === tabId)
}

function isLockedTab(ws: ProjectWorkspace, tabId: TabId): boolean {
  return findTab(ws, tabId)?.locked ?? false
}

function resetToSinglePane(ws: ProjectWorkspace) {
  const allTabIds = ws.tabs.map((tab) => tab.id)
  ws.panes = [createPane('sole')]
  ws.panes[0].tabs = allTabIds
  ws.panes[0].activeTabId = allTabIds[allTabIds.length - 1] ?? null
  ws.splitMode = 'single'
  ws.quadLayout = null
  focusedPaneSlot = 'sole'
}

function removeTabIds(ws: ProjectWorkspace, tabIds: Set<TabId>) {
  if (tabIds.size === 0) return

  for (const tabId of tabIds) {
    const tab = findTab(ws, tabId)
    if (tab?.kind === 'session') {
      sessionRegistry.dispose(tab.sessionId)
    }
  }

  for (const pane of ws.panes) {
    setPaneTabs(
      pane,
      pane.tabs.filter((id) => !tabIds.has(id))
    )
  }

  ws.tabs = ws.tabs.filter((tab) => !tabIds.has(tab.id))
}

// ---------------------------------------------------------------------------
// Project tab operations
// ---------------------------------------------------------------------------

export function getOpenProjectIds(): string[] {
  return openProjectIds
}

export function getActiveProjectId(): string | null {
  return activeProjectId
}

export function openProject(projectId: string) {
  if (!openProjectIds.includes(projectId)) {
    openProjectIds = [...openProjectIds, projectId]
  }
  ensureWorkspace(projectId)
  activeProjectId = projectId
}

export function closeProject(projectId: string) {
  // Dispose all session PTYs for this project
  const ws = getWorkspace(projectId)
  if (ws) {
    for (const tab of ws.tabs) {
      if (tab.kind === 'session') {
        sessionRegistry.dispose(tab.sessionId)
      }
    }
    const next = new Map(workspaces)
    next.delete(projectId)
    workspaces = next
  }

  // Stop git polling and clear cached summary
  clearGitState(projectId)

  openProjectIds = openProjectIds.filter((id) => id !== projectId)

  if (activeProjectId === projectId) {
    activeProjectId = openProjectIds.length > 0 ? openProjectIds[openProjectIds.length - 1] : null
  }
}

export function selectProject(projectId: string | null) {
  if (projectId && openProjectIds.includes(projectId)) {
    activeProjectId = projectId
  } else if (projectId === null) {
    activeProjectId = null
  }
}

export function reorderProjects(fromIndex: number, toIndex: number) {
  if (fromIndex < 0 || toIndex < 0 || fromIndex >= openProjectIds.length || toIndex >= openProjectIds.length) {
    return
  }
  const next = [...openProjectIds]
  const [moved] = next.splice(fromIndex, 1)
  next.splice(toIndex, 0, moved)
  openProjectIds = next
}

// ---------------------------------------------------------------------------
// Tab operations
// ---------------------------------------------------------------------------

export function addSessionTab(projectId: string, sessionId: string, title: string, providerId: string): TabId {
  const ws = ensureWorkspace(projectId)
  // Check if a tab for this session already exists
  const existing = ws.tabs.find((t) => t.kind === 'session' && t.sessionId === sessionId)
  if (existing) {
    // Activate it instead of creating a duplicate
    const pane = findPaneForTab(ws, existing.id) ?? activePaneOf(ws)
    pane.activeTabId = existing.id
    focusedPaneSlot = pane.slot
    commitWorkspace(ws)
    return existing.id
  }

  const tab: SessionTab = {
    kind: 'session',
    id: generateTabId(),
    sessionId,
    title,
    providerId,
    locked: false
  }

  ws.tabs = [...ws.tabs, tab]
  const pane = activePaneOf(ws)
  pane.tabs = [...pane.tabs, tab.id]
  pane.activeTabId = tab.id
  focusedPaneSlot = pane.slot
  commitWorkspace(ws)

  return tab.id
}

export function addDiffTab(projectId: string, filePath: string, context: DiffContext, temporary: boolean): TabId {
  const ws = ensureWorkspace(projectId)
  const pane = activePaneOf(ws)

  // If temporary, replace existing temporary diff tab in this pane
  if (temporary) {
    const existingTempId = pane.tabs.find((id) => {
      const tab = findTab(ws, id)
      return tab?.kind === 'diff' && tab.temporary
    })

    if (existingTempId) {
      // Update the existing temporary tab in place
      ws.tabs = ws.tabs.map((t) =>
        t.id === existingTempId
          ? {
              kind: 'diff' as const,
              id: existingTempId,
              filePath,
              context,
              temporary: true,
              locked: t.kind === 'diff' ? t.locked : false
            }
          : t
      )
      pane.activeTabId = existingTempId
      commitWorkspace(ws)
      return existingTempId
    }
  }

  // Check if a persistent tab for this file already exists
  const existing = ws.tabs.find((t) => t.kind === 'diff' && t.filePath === filePath && !t.temporary)
  if (existing) {
    const existingPane = findPaneForTab(ws, existing.id) ?? pane
    existingPane.activeTabId = existing.id
    focusedPaneSlot = existingPane.slot
    commitWorkspace(ws)
    return existing.id
  }

  const tab: DiffTab = {
    kind: 'diff',
    id: generateTabId(),
    filePath,
    context,
    temporary,
    locked: false
  }

  ws.tabs = [...ws.tabs, tab]
  pane.tabs = [...pane.tabs, tab.id]
  pane.activeTabId = tab.id
  focusedPaneSlot = pane.slot
  commitWorkspace(ws)

  return tab.id
}

export function promoteTemporaryTab(tabId: TabId) {
  const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined
  if (!ws) return

  ws.tabs = ws.tabs.map((t) => (t.id === tabId && t.kind === 'diff' && t.temporary ? { ...t, temporary: false } : t))
  commitWorkspace(ws)
}

export function closeTab(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const tab = findTab(ws, tabId)
  if (!tab) return
  if (tab.locked) return

  // Dispose the frontend terminal manager. The PTY should already be
  // stopped by the caller (PaneTabBar.handleTabClose), but dispose
  // ensures the xterm instance and channels are cleaned up.
  if (tab.kind === 'session') {
    sessionRegistry.dispose(tab.sessionId)
  }

  // Remove from pane
  for (const pane of ws.panes) {
    if (pane.tabs.includes(tabId)) {
      removeTabFromPane(pane, tabId)
      break
    }
  }

  // Remove from workspace tabs
  ws.tabs = ws.tabs.filter((t) => t.id !== tabId)
  commitWorkspace(ws)
}

/** Close a tab by its sessionId (used when archiving). Skips locked tabs. */
export function closeTabBySessionId(projectId: string, sessionId: string) {
  const ws = getWorkspace(projectId)
  if (!ws) return
  const tab = ws.tabs.find((t) => t.kind === 'session' && t.sessionId === sessionId)
  if (tab) closeTab(projectId, tab.id)
}

export function setActiveTab(projectId: string, paneSlot: PaneSlot, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const pane = ws.panes.find((p) => p.slot === paneSlot)
  if (pane && pane.tabs.includes(tabId)) {
    pane.activeTabId = tabId
    focusedPaneSlot = pane.slot
    commitWorkspace(ws)
  }
}

export function focusTab(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const pane = findPaneForTab(ws, tabId)
  if (!pane) return

  pane.activeTabId = tabId
  focusedPaneSlot = pane.slot
  commitWorkspace(ws)
}

export function focusSessionTab(sessionId: string) {
  const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined
  if (!ws) return

  const tab = ws.tabs.find((candidate) => candidate.kind === 'session' && candidate.sessionId === sessionId)
  if (!tab) return

  focusTab(ws.projectId, tab.id)
}

export function getAllTabs(projectId: string): Tab[] {
  return getWorkspace(projectId)?.tabs ?? []
}

// ---------------------------------------------------------------------------
// Pane focus
// ---------------------------------------------------------------------------

export function getFocusedPaneSlot(): PaneSlot {
  return focusedPaneSlot
}

export function setFocusedPane(slot: PaneSlot) {
  focusedPaneSlot = slot
}

export function getFocusedTab(projectId: string): Tab | null {
  const ws = getWorkspace(projectId)
  if (!ws) return null

  const pane = ws.panes.find((candidate) => candidate.slot === focusedPaneSlot) ?? ws.panes[0]
  if (!pane?.activeTabId) return null

  return findTab(ws, pane.activeTabId) ?? null
}

export function getFocusedDiffTab(projectId: string): DiffTab | null {
  const tab = getFocusedTab(projectId)
  return tab?.kind === 'diff' ? tab : null
}

function paneSlots(ws: ProjectWorkspace): Set<PaneSlot> {
  return new Set(ws.panes.map((pane) => pane.slot))
}

function deriveQuadLayout(ws: ProjectWorkspace): QuadLayout {
  if (ws.panes.length !== 3) return null

  const slots = paneSlots(ws)
  if (!slots.has('top-right') && slots.has('top-left') && slots.has('bottom-left') && slots.has('bottom-right')) {
    return 'top'
  }
  if (!slots.has('top-left') && slots.has('top-right') && slots.has('bottom-left') && slots.has('bottom-right')) {
    return 'bottom'
  }
  if (!slots.has('bottom-left') && slots.has('top-left') && slots.has('top-right') && slots.has('bottom-right')) {
    return 'left'
  }
  if (!slots.has('bottom-right') && slots.has('top-left') && slots.has('top-right') && slots.has('bottom-left')) {
    return 'right'
  }

  return ws.quadLayout
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
export function canSplitPane(projectId: string, paneSlot: PaneSlot, direction: 'right' | 'down'): boolean {
  const ws = getWorkspace(projectId)
  if (!ws) return false

  if (ws.splitMode === 'single') return true
  if (ws.splitMode === 'horizontal') return direction === 'down' && (paneSlot === 'left' || paneSlot === 'right')
  if (ws.splitMode === 'vertical') return direction === 'right' && (paneSlot === 'left' || paneSlot === 'right')
  if (ws.splitMode !== 'quad' || ws.panes.length >= 4) return false

  if (ws.quadLayout === 'top') {
    return paneSlot === 'top-left' && direction === 'right'
  }
  if (ws.quadLayout === 'bottom') {
    return paneSlot === 'bottom-left' && direction === 'right'
  }
  if (ws.quadLayout === 'left') {
    return paneSlot === 'top-left' && direction === 'down'
  }
  if (ws.quadLayout === 'right') {
    return paneSlot === 'top-right' && direction === 'down'
  }

  return false
}

export function splitPaneAt(projectId: string, paneSlot: PaneSlot, direction: 'right' | 'down'): PaneSlot | null {
  const ws = getWorkspace(projectId)
  if (!ws) return null
  if (!canSplitPane(projectId, paneSlot, direction)) return null

  let newSlot: PaneSlot

  if (ws.splitMode === 'single') {
    ws.panes[0].slot = 'left'
    ws.panes = [ws.panes[0], createPane('right')]
    ws.splitMode = direction === 'right' ? 'horizontal' : 'vertical'
    ws.quadLayout = null
    newSlot = 'right'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.splitMode === 'horizontal') {
    const left = ws.panes.find((pane) => pane.slot === 'left')
    const right = ws.panes.find((pane) => pane.slot === 'right')
    if (!left || !right) return null

    left.slot = 'top-left'
    right.slot = 'top-right'
    ws.panes = [left, right, createPane(paneSlot === 'left' ? 'bottom-left' : 'bottom-right')]
    ws.splitMode = 'quad'
    ws.quadLayout = paneSlot === 'left' ? 'right' : 'left'
    newSlot = paneSlot === 'left' ? 'bottom-left' : 'bottom-right'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.splitMode === 'vertical') {
    const top = ws.panes.find((pane) => pane.slot === 'left')
    const bottom = ws.panes.find((pane) => pane.slot === 'right')
    if (!top || !bottom) return null

    top.slot = 'top-left'
    bottom.slot = 'bottom-left'
    ws.panes = [top, bottom, createPane(paneSlot === 'left' ? 'top-right' : 'bottom-right')]
    ws.splitMode = 'quad'
    ws.quadLayout = paneSlot === 'left' ? 'bottom' : 'top'
    newSlot = paneSlot === 'left' ? 'top-right' : 'bottom-right'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'top' && paneSlot === 'top-left' && direction === 'right') {
    ws.panes = [...ws.panes, createPane('top-right')]
    ws.quadLayout = null
    newSlot = 'top-right'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'bottom' && paneSlot === 'bottom-left' && direction === 'right') {
    ws.panes = [...ws.panes, createPane('bottom-right')]
    ws.quadLayout = null
    newSlot = 'bottom-right'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'left' && paneSlot === 'top-left' && direction === 'down') {
    ws.panes = [...ws.panes, createPane('bottom-left')]
    ws.quadLayout = null
    newSlot = 'bottom-left'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'right' && paneSlot === 'top-right' && direction === 'down') {
    ws.panes = [...ws.panes, createPane('bottom-right')]
    ws.quadLayout = null
    newSlot = 'bottom-right'
    focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  return null
}

/**
 * Move a tab from its current pane to a target pane slot.
 * If the target pane doesn't exist, creates it via split.
 */
export function moveTabToPane(projectId: string, tabId: TabId, targetSlot: PaneSlot) {
  const ws = getWorkspace(projectId)
  if (!ws) return
  if (isLockedTab(ws, tabId)) return

  const sourcePane = findPaneForTab(ws, tabId)
  if (sourcePane?.slot === targetSlot) {
    sourcePane.activeTabId = tabId
    commitWorkspace(ws)
    return
  }

  // Remove from current pane
  for (const pane of ws.panes) {
    if (!pane.tabs.includes(tabId)) continue
    removeTabFromPane(pane, tabId)
    break
  }

  // Add to target pane
  let targetPane = ws.panes.find((p) => p.slot === targetSlot)
  if (!targetPane) {
    targetPane = createPane(targetSlot)
    ws.panes = [...ws.panes, targetPane]
  }
  targetPane.tabs = [...targetPane.tabs, tabId]
  targetPane.activeTabId = tabId
  commitWorkspace(ws)
  collapsePaneIfEmpty(projectId)
}

/**
 * Collapse empty panes and simplify split mode.
 * Called after a tab is closed or moved.
 */
export function collapsePaneIfEmpty(projectId: string) {
  const ws = getWorkspace(projectId)
  if (!ws) return
  const previousSplitMode = ws.splitMode

  if (ws.tabs.length === 0) {
    resetToSinglePane(ws)
    commitWorkspace(ws)
    return
  }

  // Remove empty panes (keep at least one)
  const nonEmpty = ws.panes.filter((p) => p.tabs.length > 0)
  if (nonEmpty.length === 0) {
    resetToSinglePane(ws)
    commitWorkspace(ws)
    return
  }

  ws.panes = nonEmpty

  // Simplify mode based on remaining pane count
  if (nonEmpty.length === 1) {
    nonEmpty[0].slot = 'sole'
    ws.splitMode = 'single'
    ws.quadLayout = null
    focusedPaneSlot = 'sole'
  } else if (nonEmpty.length === 2) {
    const slots = new Set(nonEmpty.map((pane) => pane.slot))
    const genericPair = slots.has('left') && slots.has('right')
    const horizontal =
      (slots.has('top-left') && slots.has('top-right')) || (slots.has('bottom-left') && slots.has('bottom-right'))
    const vertical =
      (slots.has('top-left') && slots.has('bottom-left')) || (slots.has('top-right') && slots.has('bottom-right'))

    const nextSplitMode =
      genericPair && (previousSplitMode === 'horizontal' || previousSplitMode === 'vertical')
        ? previousSplitMode
        : vertical
          ? 'vertical'
          : 'horizontal'

    nonEmpty[0].slot = 'left'
    nonEmpty[1].slot = 'right'
    ws.splitMode = nextSplitMode
    ws.quadLayout = null
    if (focusedPaneSlot !== 'left' && focusedPaneSlot !== 'right') {
      focusedPaneSlot = 'left'
    }
  } else if (nonEmpty.length === 3) {
    ws.splitMode = 'quad'
    ws.quadLayout = deriveQuadLayout(ws)
  } else {
    ws.splitMode = 'quad'
    ws.quadLayout = null
  }

  commitWorkspace(ws)
}

export function getPanes(projectId: string): PaneState[] {
  return getWorkspace(projectId)?.panes ?? []
}

export function getSplitMode(projectId: string): SplitMode {
  return getWorkspace(projectId)?.splitMode ?? 'single'
}

export function getQuadLayout(projectId: string): QuadLayout {
  return getWorkspace(projectId)?.quadLayout ?? null
}

export function syncSessionTabs(projectId: string, sessions: Session[]) {
  const ws = ensureWorkspace(projectId)
  const nextSessionIds = new Set(sessions.map((session) => session.id))
  const removedTabIds = new Set(
    ws.tabs.filter((tab) => tab.kind === 'session' && !nextSessionIds.has(tab.sessionId)).map((tab) => tab.id)
  )

  removeTabIds(ws, removedTabIds)

  ws.tabs = ws.tabs.map((tab) => {
    if (tab.kind !== 'session') return tab
    const session = sessions.find((candidate) => candidate.id === tab.sessionId)
    if (!session) return tab
    if (tab.title === session.title && tab.providerId === session.provider_id) return tab
    return {
      ...tab,
      title: session.title,
      providerId: session.provider_id
    }
  })

  const existingSessionIds = new Set(ws.tabs.filter((tab) => tab.kind === 'session').map((tab) => tab.sessionId))
  const targetPane = activePaneOf(ws)
  let activated = ws.panes.some((pane) => pane.activeTabId !== null)

  for (const session of sessions) {
    if (existingSessionIds.has(session.id)) continue
    if (!ACTIVE_STATUSES.has(session.status)) continue

    const tab: SessionTab = {
      kind: 'session',
      id: generateTabId(),
      sessionId: session.id,
      title: session.title,
      providerId: session.provider_id,
      locked: false
    }

    ws.tabs = [...ws.tabs, tab]
    targetPane.tabs = [...targetPane.tabs, tab.id]
    if (!activated) {
      targetPane.activeTabId = tab.id
      activated = true
    }
  }

  for (const pane of ws.panes) {
    ensureActiveTab(pane)
  }

  // Single commit — collapsePaneIfEmpty also commits, so only call it
  // when there are genuinely empty panes to avoid a redundant clone.
  const hasEmptyPanes = ws.panes.some((pane) => pane.tabs.length === 0)
  commitWorkspace(ws)
  if (hasEmptyPanes) {
    collapsePaneIfEmpty(projectId)
  }
}

export function startTabDrag(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws || isLockedTab(ws, tabId)) return
  draggedTab = { projectId, tabId }
}

export function getDraggedTab(): DraggedTab | null {
  return draggedTab
}

export function endTabDrag() {
  draggedTab = null
}

export function toggleTabLocked(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  ws.tabs = ws.tabs.map((tab) => (tab.id === tabId ? { ...tab, locked: !tab.locked } : tab))
  commitWorkspace(ws)
}

// ---------------------------------------------------------------------------
// Derived session identity
// ---------------------------------------------------------------------------

/** The active session is the session tab that's active in the focused pane. */
export function getActiveSessionId(): string | null {
  const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined
  if (!ws) return null

  const pane = ws.panes.find((p) => p.slot === focusedPaneSlot) ?? ws.panes[0]
  if (!pane?.activeTabId) return null

  const tab = ws.tabs.find((t) => t.id === pane.activeTabId)
  return tab?.kind === 'session' ? tab.sessionId : null
}
