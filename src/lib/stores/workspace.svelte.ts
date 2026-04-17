// Workspace state module — central tab + pane layout state.
//
// Manages which projects are open as tabs, which session/diff tabs exist
// within each project, and which pane each tab is assigned to.
// Replaces activeProjectId (from projects store) and activeSessionId
// (from sessions store) with a richer per-pane model.

import type { Session } from '$lib/types/backend'
import * as sessionRegistry from '$lib/terminal/sessionRegistry'
import { clearGitState } from '$lib/stores/git.svelte'
import {
  deserializeWorkspace,
  flushAllWorkspaces,
  flushWorkspace,
  loadPersistedAppShell,
  loadPersistedWorkspace,
  schedulePersistAppShell,
  schedulePersistWorkspace,
  serializeWorkspace
} from '$lib/stores/workspacePersistence'

// Statuses that warrant a legacy bootstrap tab — used only when no
// persisted workspace blob exists for the project (i.e. first-time
// open after upgrading from a pre-recovery build). Once a workspace
// has been restored we trust the saved layout and let the user
// re-open historical sessions explicitly.
const BOOTSTRAP_STATUSES = new Set(['idle', 'starting', 'running'])

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

export interface CommitTab {
  kind: 'commit'
  id: TabId
  commitHash: string
  shortHash: string
  message: string
  initialFile: string | null
  temporary: boolean
  locked: boolean
}

export interface ChangesTab {
  kind: 'changes'
  id: TabId
  label: string
  staged: boolean
  initialFile: string | null
  temporary: boolean
  locked: boolean
}

export interface StashTab {
  kind: 'stash'
  id: TabId
  stashIndex: number
  message: string
  initialFile: string | null
  temporary: boolean
  locked: boolean
}

export interface EditorTab {
  kind: 'editor'
  id: TabId
  filePath: string
  fileName: string
  temporary: boolean
  locked: boolean
  /** Git ref for read-only snapshots (e.g. "abc1234" or "stash@{0}"). */
  gitRef?: string
  /** Display label for the snapshot (e.g. "abc1234" or "stash-0"). */
  refLabel?: string
}

export interface NotificationTestTab {
  kind: 'notification-test'
  id: TabId
  label: string
  temporary: boolean
  locked: boolean
}

export type Tab = SessionTab | CommitTab | ChangesTab | StashTab | EditorTab | NotificationTestTab

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
  // Per-project pane focus. Lived on the global scope in earlier
  // versions, which meant switching projects silently applied the
  // other project's focus slot to layouts that didn't have that slot.
  focusedPaneSlot: PaneSlot
}

export interface DraggedTab {
  projectId: string
  tabId: TabId
}

// ---------------------------------------------------------------------------
// Persisted shapes (versioned)
// ---------------------------------------------------------------------------

export type PersistedTab =
  | { kind: 'session'; sessionId: string; title: string; providerId: string; locked: boolean }
  | {
      kind: 'editor'
      filePath: string
      gitRef?: string
      refLabel?: string
      temporary: boolean
      locked: boolean
    }
  | {
      kind: 'commit'
      commitHash: string
      shortHash: string
      message: string
      initialFile: string | null
      temporary: boolean
      locked: boolean
    }
  | {
      kind: 'changes'
      staged: boolean
      initialFile: string | null
      temporary: boolean
      locked: boolean
    }
  | {
      kind: 'stash'
      stashIndex: number
      message: string
      initialFile: string | null
      temporary: boolean
      locked: boolean
    }
  | { kind: 'notification-test'; label: string; temporary: boolean; locked: boolean }

export interface PersistedWorkspaceV1 {
  version: 1
  focusedPaneSlot: PaneSlot
  splitMode: SplitMode
  quadLayout: QuadLayout
  panes: Array<{
    slot: PaneSlot
    activeTabIndex: number
    tabIndices: number[]
  }>
  tabs: PersistedTab[]
}

// ---------------------------------------------------------------------------
// Module state
// ---------------------------------------------------------------------------

let openProjectIds = $state<string[]>([])
let activeProjectId = $state<string | null>(null)
let workspaces = $state<Map<string, ProjectWorkspace>>(new Map())
let draggedTab = $state<DraggedTab | null>(null)

// Active project's focused pane slot — derived so reads stay reactive
// and per-project focus is preserved when switching projects.
function activeFocusedSlot(): PaneSlot {
  const ws = activeProjectId ? workspaces.get(activeProjectId) : undefined
  return ws?.focusedPaneSlot ?? 'sole'
}

// Restore state per project. `null` = restored (no promise pending);
// `Promise` = in-flight. Absence from the map = never started.
// Until a project is restored we suppress persistence and the legacy
// session-tab bootstrap so we don't clobber state mid-restore.
const restoreState = new Map<string, Promise<void> | null>()

function isProjectRestored(projectId: string): boolean {
  return restoreState.get(projectId) === null
}

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
 *
 * After committing, schedules a debounced workspace persist. This is
 * the single choke point for layout mutation, so persisting here is
 * the cheapest way to keep disk in sync with memory.
 */
function commitWorkspace(ws: ProjectWorkspace) {
  workspaces = new Map(workspaces).set(ws.projectId, {
    ...ws,
    tabs: [...ws.tabs],
    panes: ws.panes.map(clonePane)
  })
  if (isProjectRestored(ws.projectId)) {
    const projectId = ws.projectId
    schedulePersistWorkspace(projectId, () => {
      // Skip writes for projects that were closed between the schedule
      // and the debounce flush — the project row may be gone, and even
      // if it were still present, resurrecting its old layout is wrong.
      const latest = workspaces.get(projectId)
      if (!latest) return null
      return serializeWorkspace(latest, latest.focusedPaneSlot)
    })
  }
}

function persistAppShellSnapshot() {
  schedulePersistAppShell({
    openProjectIds: [...openProjectIds],
    activeProjectId
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
      quadLayout: null,
      focusedPaneSlot: 'sole'
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
  return ws.panes.find((p) => p.slot === ws.focusedPaneSlot) ?? ws.panes[0]
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
  ws.focusedPaneSlot = 'sole'
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
  persistAppShellSnapshot()
  // Restore in the background. Failures are swallowed inside loader
  // so the UI never blocks waiting on disk.
  void restoreWorkspaceFromDisk(projectId)
}

export async function closeProject(projectId: string): Promise<void> {
  // Persist any pending mutations before we tear down — closing a
  // project mid-typing would otherwise lose the last debounce window.
  // Awaited so the producer runs against a still-live workspace entry;
  // otherwise it'd skip the write when it saw an already-deleted map.
  await flushWorkspace(projectId)

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

  clearGitState(projectId)

  openProjectIds = openProjectIds.filter((id) => id !== projectId)
  restoreState.delete(projectId)

  if (activeProjectId === projectId) {
    activeProjectId = openProjectIds.length > 0 ? openProjectIds[openProjectIds.length - 1] : null
  }
  persistAppShellSnapshot()
}

export function selectProject(projectId: string | null) {
  if (projectId && openProjectIds.includes(projectId)) {
    activeProjectId = projectId
  } else if (projectId === null) {
    activeProjectId = null
  }
  persistAppShellSnapshot()
}

export function reorderProjects(fromIndex: number, toIndex: number) {
  if (fromIndex < 0 || toIndex < 0 || fromIndex >= openProjectIds.length || toIndex >= openProjectIds.length) {
    return
  }
  const next = [...openProjectIds]
  const [moved] = next.splice(fromIndex, 1)
  next.splice(toIndex, 0, moved)
  openProjectIds = next
  persistAppShellSnapshot()
}

// Idempotent per project: the first caller does the work and marks
// the project restored; concurrent callers share the in-flight
// promise; later callers return immediately. Once restored, commits
// start persisting and the legacy bootstrap heuristic is suppressed.
export async function restoreWorkspaceFromDisk(projectId: string): Promise<void> {
  const existing = restoreState.get(projectId)
  if (existing === null) return
  if (existing) return existing

  // IIFE catches its own errors: the outer promise resolves cleanly
  // so `void restoreWorkspaceFromDisk(...)` callers never produce an
  // unhandled rejection, and a malformed blob falls back to an empty
  // workspace instead of poisoning `restoreState`.
  const promise = (async () => {
    try {
      const persisted = await loadPersistedWorkspace(projectId)
      if (!persisted) return

      const hydrated = deserializeWorkspace(persisted, generateTabId)
      workspaces = new Map(workspaces).set(projectId, {
        projectId,
        tabs: hydrated.tabs,
        panes: hydrated.panes,
        splitMode: hydrated.splitMode,
        quadLayout: hydrated.quadLayout,
        focusedPaneSlot: hydrated.focusedPaneSlot
      })
    } catch (error) {
      console.warn(`Workspace restore failed for ${projectId}, falling back to empty layout:`, error)
    }
  })()

  restoreState.set(projectId, promise)
  try {
    await promise
  } finally {
    restoreState.set(projectId, null)
  }
}

/**
 * Read the persisted app-shell state and reopen the projects that
 * were open last session. Validates each project id against the
 * current project list so deleted projects don't resurrect.
 *
 * Workspace hydration runs *before* the active project is selected
 * so ProjectView never mounts against an empty workspace. If we
 * exposed an active project mid-restore the legacy bootstrap
 * heuristic would fire and duplicate the persisted tabs.
 */
export async function restoreAppShellState(validProjectIds: Set<string>): Promise<void> {
  const { openProjectIds: savedOpen, activeProjectId: savedActive } = await loadPersistedAppShell()
  const filteredOpen = savedOpen.filter((id) => validProjectIds.has(id))

  if (filteredOpen.length === 0) {
    // Saved state pointed at only-deleted projects. Persist an empty
    // snapshot so the next launch doesn't replay the same dead ids.
    if (savedOpen.length > 0 || savedActive !== null) {
      persistAppShellSnapshot()
    }
    return
  }

  openProjectIds = filteredOpen
  for (const id of filteredOpen) {
    ensureWorkspace(id)
  }

  // Hydrate every workspace before flipping the active project so
  // ProjectView never mounts against empty state and triggers the
  // legacy bootstrap heuristic.
  await Promise.all(filteredOpen.map((id) => restoreWorkspaceFromDisk(id)))

  const desiredActive = savedActive && filteredOpen.includes(savedActive) ? savedActive : (filteredOpen[0] ?? null)
  activeProjectId = desiredActive
  persistAppShellSnapshot()
}

/**
 * Force an immediate write of every pending workspace + app-shell
 * mutation. Intended for the managed reload path so a Cmd+R doesn't
 * race the debounce window.
 */
export async function flushPersistencePending(): Promise<void> {
  await flushAllWorkspaces()
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
    ws.focusedPaneSlot = pane.slot
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
  ws.focusedPaneSlot = pane.slot
  commitWorkspace(ws)

  return tab.id
}

/** Compare the content-bearing fields of two tabs (ignoring id/locked). */
function tabDataChanged(a: Tab, b: Tab): boolean {
  if (a.kind !== b.kind) return true
  switch (a.kind) {
    case 'commit':
      return b.kind !== 'commit' || a.commitHash !== b.commitHash || a.initialFile !== b.initialFile
    case 'changes':
      return b.kind !== 'changes' || a.staged !== b.staged || a.initialFile !== b.initialFile
    case 'stash':
      return b.kind !== 'stash' || a.stashIndex !== b.stashIndex || a.initialFile !== b.initialFile
    case 'editor':
      return b.kind !== 'editor' || a.filePath !== b.filePath || a.gitRef !== b.gitRef
    case 'notification-test':
      return b.kind !== 'notification-test' || a.label !== b.label
    default:
      return true
  }
}

/**
 * Generic content-tab helper. Handles the 3-phase pattern:
 * 1. Replace existing temporary tab of same kind in active pane
 * 2. Reuse an existing persistent tab (optional update via `onReuse`)
 * 3. Create a new tab
 */
function addContentTab(
  projectId: string,
  kind: Tab['kind'],
  makeTab: (id: TabId) => Tab,
  temporary: boolean,
  matchPersistent?: (t: Tab) => boolean,
  onReuse?: (existing: Tab) => Tab
): TabId {
  const ws = ensureWorkspace(projectId)
  const pane = activePaneOf(ws)

  // Phase 1: replace existing temporary tab of same kind in this pane
  if (temporary) {
    const existingTempId = pane.tabs.find((id) => {
      const t = findTab(ws, id)
      return t?.kind === kind && t.kind !== 'session' && t.temporary
    })

    if (existingTempId) {
      const newTab = makeTab(existingTempId)
      const existingTab = findTab(ws, existingTempId)
      // Skip workspace mutation when tab data hasn't changed
      // (e.g. second click of a double-click on the same file)
      if (existingTab && !tabDataChanged(existingTab, newTab)) {
        return existingTempId
      }
      ws.tabs = ws.tabs.map((t) => (t.id === existingTempId ? newTab : t))
      pane.activeTabId = existingTempId
      commitWorkspace(ws)
      return existingTempId
    }
  }

  // Phase 2: reuse existing persistent tab
  if (matchPersistent) {
    const existing = ws.tabs.find((t) => matchPersistent(t))
    if (existing) {
      if (onReuse) {
        const updated = onReuse(existing)
        if (updated !== existing) {
          ws.tabs = ws.tabs.map((t) => (t.id === existing.id ? updated : t))
        }
      }
      const existingPane = findPaneForTab(ws, existing.id) ?? pane
      existingPane.activeTabId = existing.id
      ws.focusedPaneSlot = existingPane.slot
      commitWorkspace(ws)
      return existing.id
    }
  }

  // Phase 3: create new tab
  const tab = makeTab(generateTabId())
  ws.tabs = [...ws.tabs, tab]
  pane.tabs = [...pane.tabs, tab.id]
  pane.activeTabId = tab.id
  ws.focusedPaneSlot = pane.slot
  commitWorkspace(ws)

  return tab.id
}

export function addCommitTab(
  projectId: string,
  commitHash: string,
  shortHash: string,
  message: string,
  initialFile: string | null = null,
  temporary = true
): TabId {
  return addContentTab(
    projectId,
    'commit',
    (id): CommitTab => ({ kind: 'commit', id, commitHash, shortHash, message, initialFile, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'commit' && t.commitHash === commitHash && !t.temporary,
    (t) => (t.kind === 'commit' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addChangesTab(
  projectId: string,
  staged: boolean,
  initialFile: string | null = null,
  temporary = true
): TabId {
  const label = staged ? 'Staged Changes' : 'Changes'
  return addContentTab(
    projectId,
    'changes',
    (id): ChangesTab => ({ kind: 'changes', id, label, staged, initialFile, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'changes' && t.staged === staged && !t.temporary,
    (t) => (t.kind === 'changes' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addStashTab(
  projectId: string,
  stashIndex: number,
  message: string,
  initialFile: string | null = null,
  temporary = true
): TabId {
  return addContentTab(
    projectId,
    'stash',
    (id): StashTab => ({ kind: 'stash', id, stashIndex, message, initialFile, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'stash' && t.stashIndex === stashIndex && !t.temporary,
    (t) => (t.kind === 'stash' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addEditorTab(projectId: string, filePath: string, temporary = true): TabId {
  const fileName = filePath.split('/').pop() ?? filePath
  return addContentTab(
    projectId,
    'editor',
    (id): EditorTab => ({ kind: 'editor', id, filePath, fileName, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'editor' && t.filePath === filePath && !t.gitRef && !t.temporary
  )
}

/** Open a read-only editor tab showing a file at a specific git revision. */
export function addReadonlyEditorTab(
  projectId: string,
  filePath: string,
  gitRef: string,
  refLabel: string,
  temporary = true
): TabId {
  const fileName = filePath.split('/').pop() ?? filePath
  return addContentTab(
    projectId,
    'editor',
    (id): EditorTab => ({ kind: 'editor', id, filePath, fileName, temporary, locked: false, gitRef, refLabel }),
    temporary,
    (t) => t.kind === 'editor' && t.filePath === filePath && t.gitRef === gitRef && !t.temporary
  )
}

export function addNotificationTestTab(projectId: string, temporary = false): TabId {
  return addContentTab(
    projectId,
    'notification-test',
    (id): NotificationTestTab => ({
      kind: 'notification-test',
      id,
      label: 'Notification Tester',
      temporary,
      locked: false
    }),
    temporary,
    (t) => t.kind === 'notification-test' && !t.temporary
  )
}

export function promoteTemporaryTab(tabId: TabId) {
  const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined
  if (!ws) return

  ws.tabs = ws.tabs.map((t) => {
    if (t.id !== tabId || t.kind === 'session') return t
    if (t.temporary) return { ...t, temporary: false }
    return t
  })
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
    ws.focusedPaneSlot = pane.slot
    commitWorkspace(ws)
  }
}

export function focusTab(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const pane = findPaneForTab(ws, tabId)
  if (!pane) return

  pane.activeTabId = tabId
  ws.focusedPaneSlot = pane.slot
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
  return activeFocusedSlot()
}

export function setFocusedPane(slot: PaneSlot) {
  const ws = activeProjectId ? workspaces.get(activeProjectId) : undefined
  if (!ws) return
  if (ws.focusedPaneSlot === slot) return
  ws.focusedPaneSlot = slot
  // A focus change is a commit-worthy mutation on its own: persistence
  // captures focus from ws, so without committing here the next tab
  // mutation would ship the stale slot.
  commitWorkspace(ws)
}

export function getFocusedTab(projectId: string): Tab | null {
  const ws = getWorkspace(projectId)
  if (!ws) return null

  const pane = ws.panes.find((candidate) => candidate.slot === ws.focusedPaneSlot) ?? ws.panes[0]
  if (!pane?.activeTabId) return null

  return findTab(ws, pane.activeTabId) ?? null
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
    ws.focusedPaneSlot = newSlot
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
    ws.focusedPaneSlot = newSlot
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
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'top' && paneSlot === 'top-left' && direction === 'right') {
    ws.panes = [...ws.panes, createPane('top-right')]
    ws.quadLayout = null
    newSlot = 'top-right'
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'bottom' && paneSlot === 'bottom-left' && direction === 'right') {
    ws.panes = [...ws.panes, createPane('bottom-right')]
    ws.quadLayout = null
    newSlot = 'bottom-right'
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'left' && paneSlot === 'top-left' && direction === 'down') {
    ws.panes = [...ws.panes, createPane('bottom-left')]
    ws.quadLayout = null
    newSlot = 'bottom-left'
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'right' && paneSlot === 'top-right' && direction === 'down') {
    ws.panes = [...ws.panes, createPane('bottom-right')]
    ws.quadLayout = null
    newSlot = 'bottom-right'
    ws.focusedPaneSlot = newSlot
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
    ws.focusedPaneSlot = 'sole'
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
    if (ws.focusedPaneSlot !== 'left' && ws.focusedPaneSlot !== 'right') {
      ws.focusedPaneSlot = 'left'
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

/**
 * Reconcile session tabs against the live sessions DB.
 *
 * Removes tabs whose sessions no longer exist and updates titles for
 * tabs whose sessions were renamed. The auto-create heuristic — adding
 * tabs for any active session — only fires when no persisted workspace
 * exists yet (legacy bootstrap), so we never silently re-spawn dead
 * tab layouts on top of a restored workspace.
 */
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

  // Bootstrap heuristic: only fire when this project has *no* persisted
  // workspace blob (legacy upgrade path). Once we've consulted disk for
  // this project — whether the blob existed or not — restored tabs are
  // the source of truth. In particular, a user who intentionally closed
  // every tab must not have session tabs silently re-created on the
  // next sessions reload.
  const shouldBootstrap = !isProjectRestored(projectId)
  const existingSessionIds = new Set(ws.tabs.filter((tab) => tab.kind === 'session').map((tab) => tab.sessionId))
  const targetPane = activePaneOf(ws)
  let activated = ws.panes.some((pane) => pane.activeTabId !== null)

  if (shouldBootstrap) {
    for (const session of sessions) {
      if (existingSessionIds.has(session.id)) continue
      if (!BOOTSTRAP_STATUSES.has(session.status)) continue

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

  const pane = ws.panes.find((p) => p.slot === ws.focusedPaneSlot) ?? ws.panes[0]
  if (!pane?.activeTabId) return null

  const tab = ws.tabs.find((t) => t.id === pane.activeTabId)
  return tab?.kind === 'session' ? tab.sessionId : null
}
