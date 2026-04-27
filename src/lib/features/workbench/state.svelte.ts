// Workspace state module; central tab + pane layout state.
//
// Manages which projects are open as tabs, which session/diff tabs exist
// within each project, and which pane each tab is assigned to.
// Replaces activeProjectId (from projects store) and activeSessionId
// (from sessions store) with a richer per-pane model.

import { backend } from '$lib/api/backend'
import type { Session } from '$lib/types/backend'
import { basename } from '$lib/utils/paths'
import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
import * as taskRegistry from '$lib/features/tasks/taskRegistry'
import { clearGitState } from '$lib/features/git/state.svelte'
import { invalidateProjectFiles } from '$lib/features/files/projectFiles.svelte'
import { hideProjectPicker } from '$lib/features/app-shell/project-picker/state.svelte'
import { canLockTab, createPane } from '$lib/features/workbench/model'
import type {
  DiffSource,
  PaneSlot,
  PaneState,
  PersistedTab,
  PersistedWorkspaceV2,
  ProjectWorkspace,
  QuadLayout,
  SessionTab,
  SplitDirection,
  SplitMode,
  Tab,
  TabId,
  TaskRunStatus,
  TaskTab,
  TextTab,
  ToolTab,
  DiffTab,
  LauncherTab
} from '$lib/features/workbench/model'
import {
  deserializeWorkspace,
  flushAllWorkspaces,
  flushWorkspace,
  loadPersistedAppShell,
  loadPersistedWorkspace,
  schedulePersistAppShell,
  schedulePersistWorkspace,
  serializeWorkspace,
  tabToPersisted
} from '$lib/features/workbench/persistence'

export { canLockTab, createPane }
export type {
  DiffSource,
  PaneSlot,
  PaneState,
  PersistedTab,
  PersistedWorkspaceV2,
  ProjectWorkspace,
  QuadLayout,
  SessionTab,
  SplitDirection,
  SplitMode,
  Tab,
  TabId,
  TaskRunStatus,
  TaskTab,
  TextTab,
  ToolTab,
  DiffTab,
  LauncherTab
}

// Statuses that warrant a legacy bootstrap tab; used only when no
// persisted workspace blob exists for the project (i.e. first-time
// open after upgrading from a pre-recovery build). Once a workspace
// has been restored we trust the saved layout and let the user
// re-open historical sessions explicitly.
const BOOTSTRAP_STATUSES = new Set(['idle', 'starting', 'running'])

// MODULE STATE //
let openProjectIds = $state<string[]>([])
let activeProjectId = $state<string | null>(null)
let workspaces = $state<Map<string, ProjectWorkspace>>(new Map())
const projectCloseListeners = new Set<(projectId: string) => void>()

// LIFO per-project stack of recently closed tabs for Ctrl+Shift+T. Not
// persisted; a fresh app launch has nothing to reopen beyond what
// workspace restore already hydrates. Capped at 10 so a long session of
// tab churn doesn't balloon.
const MAX_CLOSED_TABS = 10
let closedTabs = $state<Map<string, PersistedTab[]>>(new Map())

// Monotonic counter for "Untitled-N" labels on new untitled text tabs.
// Per process only; resets across reloads which is fine; there's no
// user-meaningful numbering to preserve.
let untitledCounter = 0

// Active project's focused pane slot; derived so reads stay reactive
// and per-project focus is preserved when switching projects.
function activeFocusedSlot(): PaneSlot {
  const ws = activeProjectId ? workspaces.get(activeProjectId) : undefined
  return ws?.focusedPaneSlot ?? 'sole'
}

function notifyProjectClosed(projectId: string): void {
  for (const listener of projectCloseListeners) {
    try {
      listener(projectId)
    } catch (error) {
      console.warn('project-close-listener:', error)
    }
  }
}

// Restore state per project. `null` = restored (no promise pending);
// `Promise` = in-flight. Absence from the map = never started.
// Until a project is restored we suppress persistence and the legacy
// session-tab bootstrap so we don't clobber state mid-restore.
const restoreState = new Map<string, Promise<void> | null>()

function isProjectRestored(projectId: string): boolean {
  return restoreState.get(projectId) === null
}

// HELPERS //
let nextTabId = 0
function generateTabId(): TabId {
  return `tab-${Date.now()}-${nextTabId++}`
}

let nextRevealNonce = 0
function generateRevealNonce(): number {
  nextRevealNonce += 1
  return nextRevealNonce
}

function diffSourcesEqual(a: DiffSource, b: DiffSource): boolean {
  if (a.kind !== b.kind) return false

  switch (a.kind) {
    case 'working':
      return (
        b.kind === 'working' && a.staged === b.staged && a.scopePath === b.scopePath && a.revealNonce === b.revealNonce
      )
    case 'commit':
      return b.kind === 'commit' && a.commitHash === b.commitHash
    case 'stash':
      return b.kind === 'stash' && a.stashIndex === b.stashIndex
    default: {
      const _exhaustive: never = a
      return _exhaustive
    }
  }
}

function getWorkspace(projectId: string): ProjectWorkspace | undefined {
  return workspaces.get(projectId)
}

/**
 * Trigger Svelte reactivity after mutating a workspace in place.
 *
 * Clones the workspace, its `panes[]`, each individual pane object,
 * and each pane's `tabs[]` before swapping the outer Map. Without the
 * per-pane clone, in-place property mutations (e.g.
 * `pane.activeTabId = id` from `setActiveTab`) leave consumers reading
 * the same pane reference; Svelte's `$derived` short-circuits on
 * reference equality and the UI never picks up the change. The clone
 * is shallow per object, so it costs O(panes + total tabs) per commit.
 *
 * After committing, schedules a debounced workspace persist. This is
 * the single choke point for layout mutation.
 */
function commitWorkspace(ws: ProjectWorkspace) {
  const cloned: ProjectWorkspace = {
    ...ws,
    panes: ws.panes.map((pane) => ({ ...pane, tabs: [...pane.tabs] })),
    tabs: [...ws.tabs]
  }
  workspaces = new Map(workspaces).set(ws.projectId, cloned)
  if (isProjectRestored(ws.projectId)) {
    const projectId = ws.projectId
    schedulePersistWorkspace(projectId, () => {
      // Skip writes for projects that were closed between the schedule
      // and the debounce flush; the project row may be gone, and even
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

/**
 * Seed a launcher tab into the given pane. Used to maintain the invariant
 * that every live workspace has at least one tab; otherwise a freshly
 * opened project (or a restored workspace whose user closed everything
 * last session) would render an empty tab strip with the picker
 * floating underneath, which is visually inconsistent with every other
 * pane state and breaks the "tab strip is the source of truth" model.
 *
 * Mutates `ws` in place. Caller is responsible for committing.
 */
function seedLauncherTab(ws: ProjectWorkspace, pane: PaneState): void {
  const tab: LauncherTab = {
    kind: 'launcher',
    id: generateTabId(),
    locked: false,
    temporary: false
  }
  ws.tabs = [...ws.tabs, tab]
  pane.tabs = [...pane.tabs, tab.id]
  pane.activeTabId = tab.id
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
    // Fresh workspaces need a launcher tab so the user sees the picker as a
    // real tab (matching every other pane state) instead of a floating
    // overlay. The legacy bootstrap path in syncSessionTabs may replace
    // or extend this later with session tabs; it's additive so it
    // doesn't clobber the launcher tab.
    seedLauncherTab(ws, ws.panes[0])
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

/** Yield to the browser between dispose batches so a bulk close
 *  (e.g. closing a project's whole tab strip) doesn't freeze the UI
 *  while xterm/transcript teardown drains. */
const TAB_DISPOSE_BATCH_SIZE = 4

function runTabDisposeBatch(queue: Array<() => void>) {
  const limit = Math.min(TAB_DISPOSE_BATCH_SIZE, queue.length)
  for (let i = 0; i < limit; i++) {
    try {
      queue[i]()
    } catch (error) {
      console.warn('tab dispose:', error)
    }
  }
  if (queue.length > TAB_DISPOSE_BATCH_SIZE) {
    setTimeout(() => runTabDisposeBatch(queue.slice(TAB_DISPOSE_BATCH_SIZE)), 0)
  }
}

function removeTabIds(ws: ProjectWorkspace, tabIds: Set<TabId>) {
  if (tabIds.size === 0) return

  // Snapshot dispose targets BEFORE the tab list is mutated, but
  // defer the actual `dispose()` calls until after this function has
  // returned. Each dispose runs xterm.dispose + channel teardown +
  // transcript flush; synchronous and not free. Closing 10+ tabs at
  // once (e.g. quad-pane project close) was visibly stuttering paint.
  const disposes: Array<() => void> = []
  for (const tabId of tabIds) {
    const tab = findTab(ws, tabId)
    if (tab?.kind === 'session') {
      const sessionId = tab.sessionId
      disposes.push(() => sessionRegistry.dispose(sessionId))
    } else if (tab?.kind === 'task') {
      const runId = tab.runId
      disposes.push(() => taskRegistry.dispose(runId))
    }
  }

  for (const pane of ws.panes) {
    setPaneTabs(
      pane,
      pane.tabs.filter((id) => !tabIds.has(id))
    )
  }
  ws.tabs = ws.tabs.filter((tab) => !tabIds.has(tab.id))

  // Hand off the dispose queue to a deferred runner; `setTimeout(0)`
  // yields to paint between batches, so the tab strip's visible
  // update lands before the sessions get torn down.
  if (disposes.length > 0) {
    queueMicrotask(() => runTabDisposeBatch(disposes))
  }
}

function setClosedTabsForProject(projectId: string, tabs: PersistedTab[]) {
  const next = new Map(closedTabs)
  if (tabs.length > 0) next.set(projectId, tabs)
  else next.delete(projectId)
  closedTabs = next
}

function popClosedTabForProject(projectId: string): PersistedTab | null {
  const list = closedTabs.get(projectId)
  if (!list || list.length === 0) return null
  const [head, ...rest] = list
  setClosedTabsForProject(projectId, rest)
  return head
}

function pruneClosedSessionTabs(projectId: string, sessionIds: Set<string>) {
  const list = closedTabs.get(projectId)
  if (!list) return

  const filtered = list.filter((tab) => tab.kind !== 'session' || sessionIds.has(tab.sessionId))
  if (filtered.length === list.length) return
  setClosedTabsForProject(projectId, filtered)
}

// PROJECT TAB OPERATIONS //
export function getOpenProjectIds(): string[] {
  return openProjectIds
}

export function getActiveProjectId(): string | null {
  return activeProjectId
}

export function onProjectClosed(listener: (projectId: string) => void): () => void {
  projectCloseListeners.add(listener)
  return () => projectCloseListeners.delete(listener)
}

export function openProject(projectId: string) {
  if (!openProjectIds.includes(projectId)) {
    openProjectIds = [...openProjectIds, projectId]
  }
  ensureWorkspace(projectId)
  activeProjectId = projectId
  hideProjectPicker()
  persistAppShellSnapshot()
  // Restore in the background. Failures are swallowed inside loader
  // so the UI never blocks waiting on disk.
  void restoreWorkspaceFromDisk(projectId)
}

export async function closeProject(projectId: string): Promise<void> {
  // Persist any pending mutations before we tear down; closing a
  // project mid-typing would otherwise lose the last debounce window.
  // Awaited so the producer runs against a still-live workspace entry;
  // otherwise it'd skip the write when it saw an already-deleted map.
  await flushWorkspace(projectId)

  const ws = getWorkspace(projectId)
  if (ws) {
    for (const tab of ws.tabs) {
      if (tab.kind === 'session') {
        sessionRegistry.dispose(tab.sessionId)
        continue
      }
      if (tab.kind === 'task') {
        taskRegistry.dispose(tab.runId)
      }
    }
    const next = new Map(workspaces)
    next.delete(projectId)
    workspaces = next
  }

  clearGitState(projectId)
  invalidateProjectFiles(projectId)

  openProjectIds = openProjectIds.filter((id) => id !== projectId)
  restoreState.delete(projectId)

  if (activeProjectId === projectId) {
    activeProjectId = openProjectIds.length > 0 ? openProjectIds[openProjectIds.length - 1] : null
  }
  notifyProjectClosed(projectId)
  persistAppShellSnapshot()
}

export function selectProject(projectId: string | null) {
  if (projectId && openProjectIds.includes(projectId)) {
    ensureWorkspace(projectId)
    activeProjectId = projectId
    // Selecting an already-open project tab is an implicit "I'm done
    // with the picker"; otherwise the override keeps EmptyState up
    // even after the user pinned a real tab.
    hideProjectPicker()
    void restoreWorkspaceFromDisk(projectId)
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
      const restored: ProjectWorkspace = {
        projectId,
        tabs: hydrated.tabs,
        panes: hydrated.panes,
        splitMode: hydrated.splitMode,
        quadLayout: hydrated.quadLayout,
        focusedPaneSlot: hydrated.focusedPaneSlot
      }
      // Launcher tabs are never persisted, so a workspace saved while the
      // user had only a launcher tab open would hydrate with zero tabs.
      // Seed one back so the invariant "every workspace has at least
      // one tab" holds after restore too.
      if (restored.tabs.length === 0) {
        const targetPane = restored.panes.find((p) => p.slot === restored.focusedPaneSlot) ?? restored.panes[0]
        if (targetPane) seedLauncherTab(restored, targetPane)
      }
      workspaces = new Map(workspaces).set(projectId, restored)
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
 * Reopen projects that were open last session, filtering out any
 * project ids the user has since deleted. Disk restore for the
 * active project runs in the background so first paint is not
 * blocked on JSON parse.
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

  const desiredActive = savedActive && filteredOpen.includes(savedActive) ? savedActive : (filteredOpen[0] ?? null)
  openProjectIds = filteredOpen

  if (desiredActive) {
    ensureWorkspace(desiredActive)
    // Background restore; `commitWorkspace` skips persistence while
    // `isProjectRestored` is false, so this can't race-clobber the
    // disk blob. The seeded launcher tab covers the visible gap.
    void restoreWorkspaceFromDisk(desiredActive)
  }

  activeProjectId = desiredActive
  persistAppShellSnapshot()

  for (const id of filteredOpen) {
    if (id === desiredActive) continue
    ensureWorkspace(id)
    void restoreWorkspaceFromDisk(id)
  }
}

/**
 * Force an immediate write of every pending workspace + app-shell
 * mutation. Intended for the managed reload path so a Cmd+R doesn't
 * race the debounce window.
 */
export async function flushPersistencePending(): Promise<void> {
  await flushAllWorkspaces()
}

// TAB OPERATIONS //
export interface TaskTabInit {
  runId: string
  taskId: string
  activeFilePath: string | null
  label: string
  icon: string | null
  group: string | null
}

/**
 * Add a new task tab. Always creates a fresh tab (non-singleton
 * behavior); callers that want singleton focus-on-rerun should use
 * `findTaskTabByTaskId` first and skip the create when a live run
 * already exists.
 */
export function addTaskTab(projectId: string, init: TaskTabInit): TabId {
  const ws = ensureWorkspace(projectId)
  const tab: TaskTab = {
    kind: 'task',
    id: generateTabId(),
    runId: init.runId,
    taskId: init.taskId,
    activeFilePath: init.activeFilePath,
    label: init.label,
    icon: init.icon,
    group: init.group,
    status: 'starting',
    exitCode: null,
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

/** Find an existing task tab by its source task id. Used to enforce
 * singleton semantics on rerun: if a live tab exists for this task,
 * focus it instead of spawning a new one. */
export function findTaskTabByTaskId(projectId: string, taskId: string): TaskTab | null {
  const ws = getWorkspace(projectId)
  if (!ws) return null
  return (
    ws.tabs.find(
      (t): t is TaskTab => t.kind === 'task' && t.taskId === taskId && t.status !== 'exited' && t.status !== 'failed'
    ) ?? null
  )
}

/** Update a task tab's lifecycle status and optional exit code. */
export function setTaskTabStatus(
  projectId: string,
  tabId: TabId,
  status: TaskRunStatus,
  exitCode: number | null = null
): void {
  const ws = getWorkspace(projectId)
  if (!ws) return
  ws.tabs = ws.tabs.map((t) => {
    if (t.id !== tabId || t.kind !== 'task') return t
    return { ...t, status, exitCode }
  })
  commitWorkspace(ws)
}

/**
 * Rebind a singleton task tab to a fresh runId for restart. Resets
 * lifecycle status and caches the latest label/icon in case the task
 * definition changed since the tab was created.
 */
export function resetTaskTabForRestart(
  projectId: string,
  tabId: TabId,
  nextRunId: string,
  latest: { activeFilePath: string | null; label: string; icon: string | null; group: string | null }
): void {
  const ws = getWorkspace(projectId)
  if (!ws) return
  ws.tabs = ws.tabs.map((t) => {
    if (t.id !== tabId || t.kind !== 'task') return t
    return {
      ...t,
      runId: nextRunId,
      activeFilePath: latest.activeFilePath,
      label: latest.label,
      icon: latest.icon,
      group: latest.group,
      status: 'starting',
      exitCode: null
    }
  })
  const pane = findPaneForTab(ws, tabId) ?? activePaneOf(ws)
  pane.activeTabId = tabId
  ws.focusedPaneSlot = pane.slot
  commitWorkspace(ws)
}

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
    case 'diff':
      return b.kind !== 'diff' || !diffSourcesEqual(a.source, b.source) || a.initialFile !== b.initialFile
    case 'text':
      return b.kind !== 'text' || a.filePath !== b.filePath || a.gitRef !== b.gitRef
    case 'tool':
      return b.kind !== 'tool' || a.tool !== b.tool || a.label !== b.label
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
      return t?.kind === kind && t.kind !== 'session' && t.kind !== 'task' && t.temporary
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
    'diff',
    (id): DiffTab => ({
      kind: 'diff',
      id,
      source: { kind: 'commit', commitHash, shortHash, message },
      initialFile,
      temporary,
      locked: false
    }),
    temporary,
    (t) => t.kind === 'diff' && t.source.kind === 'commit' && t.source.commitHash === commitHash && !t.temporary,
    (t) => (t.kind === 'diff' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addChangesTab(
  projectId: string,
  staged: boolean,
  scopePath: string | null = null,
  initialFile: string | null = null,
  temporary = true
): TabId {
  return addContentTab(
    projectId,
    'diff',
    (id): DiffTab => ({
      kind: 'diff',
      id,
      source: {
        kind: 'working',
        staged,
        scopePath,
        revealNonce: generateRevealNonce()
      },
      initialFile,
      temporary,
      locked: false
    }),
    temporary,
    (t) =>
      t.kind === 'diff' &&
      t.source.kind === 'working' &&
      t.source.staged === staged &&
      t.source.scopePath === scopePath &&
      !t.temporary,
    (t) =>
      t.kind === 'diff' && t.source.kind === 'working'
        ? {
            ...t,
            source: {
              kind: 'working',
              staged,
              scopePath,
              revealNonce: generateRevealNonce()
            },
            initialFile
          }
        : t
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
    'diff',
    (id): DiffTab => ({
      kind: 'diff',
      id,
      source: { kind: 'stash', stashIndex, message },
      initialFile,
      temporary,
      locked: false
    }),
    temporary,
    (t) => t.kind === 'diff' && t.source.kind === 'stash' && t.source.stashIndex === stashIndex && !t.temporary,
    (t) => (t.kind === 'diff' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addTextTab(projectId: string, filePath: string, temporary = true): TabId {
  const fileName = basename(filePath)
  return addContentTab(
    projectId,
    'text',
    (id): TextTab => ({ kind: 'text', id, filePath, fileName, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'text' && t.filePath === filePath && !t.gitRef && !t.temporary
  )
}

/**
 * Open a new unsaved text tab ("Untitled-N"). Multiple presses stack
 * rather than dedupe; the tab promotes to a real path on first save.
 */
export function addUntitledTextTab(projectId: string): TabId {
  const ws = ensureWorkspace(projectId)
  untitledCounter += 1
  const tab: TextTab = {
    kind: 'text',
    id: generateTabId(),
    filePath: null,
    fileName: `Untitled-${untitledCounter}`,
    temporary: false,
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

/**
 * Promote an unsaved text tab (filePath=null) to a real path after
 * save-as, or rename an existing text tab's target. Used by the
 * save-dialog flow in FileEditor.
 */
export function renameTextTab(projectId: string, tabId: TabId, newFilePath: string) {
  const ws = getWorkspace(projectId)
  if (!ws) return
  const fileName = basename(newFilePath)
  ws.tabs = ws.tabs.map((t) => {
    if (t.id !== tabId || t.kind !== 'text') return t
    return { ...t, filePath: newFilePath, fileName }
  })
  commitWorkspace(ws)
}

/** Open a read-only text tab showing a file at a specific git revision. */
export function addReadonlyTextTab(
  projectId: string,
  filePath: string,
  gitRef: string,
  refLabel: string,
  temporary = true
): TabId {
  const fileName = basename(filePath)
  return addContentTab(
    projectId,
    'text',
    (id): TextTab => ({ kind: 'text', id, filePath, fileName, temporary, locked: false, gitRef, refLabel }),
    temporary,
    (t) => t.kind === 'text' && t.filePath === filePath && t.gitRef === gitRef && !t.temporary
  )
}

/**
 * Focus an existing launcher tab in the target pane, or create one if absent.
 * One launcher tab per pane maximum; the picker is a single surface, not a
 * stack of identical buffers.
 *
 * `paneSlot` defaults to the focused pane. Callers (e.g. PaneTabBar's +
 * button) pass their own slot explicitly so the picker always opens in
 * the pane the user clicked from, regardless of focus races.
 */
export function openLauncherTab(projectId: string, paneSlot?: PaneSlot): TabId {
  const ws = ensureWorkspace(projectId)
  const pane = (paneSlot && ws.panes.find((p) => p.slot === paneSlot)) || activePaneOf(ws)

  const existingLauncherId = pane.tabs.find((id) => findTab(ws, id)?.kind === 'launcher')
  if (existingLauncherId) {
    pane.activeTabId = existingLauncherId
    ws.focusedPaneSlot = pane.slot
    commitWorkspace(ws)
    return existingLauncherId
  }

  const tab: LauncherTab = {
    kind: 'launcher',
    id: generateTabId(),
    locked: false,
    temporary: false
  }

  ws.tabs = [...ws.tabs, tab]
  pane.tabs = [...pane.tabs, tab.id]
  pane.activeTabId = tab.id
  ws.focusedPaneSlot = pane.slot
  commitWorkspace(ws)
  return tab.id
}

/**
 * Close the launcher tab living in the given pane, if any. Used by
 * `workbench/Pane.svelte` to clean up after a successful session creation so the
 * picker doesn't linger next to the session it spawned.
 */
export function closeLauncherTabInPane(projectId: string, paneSlot: PaneSlot): void {
  const ws = getWorkspace(projectId)
  if (!ws) return
  const pane = ws.panes.find((p) => p.slot === paneSlot)
  if (!pane) return
  const launcherTabId = pane.tabs.find((id) => findTab(ws, id)?.kind === 'launcher')
  if (!launcherTabId) return
  closeTab(projectId, launcherTabId)
}

export function addNotificationToolTab(projectId: string, temporary = false): TabId {
  return addContentTab(
    projectId,
    'tool',
    (id): ToolTab => ({
      kind: 'tool',
      id,
      tool: 'notification-test',
      label: 'Notification Tester',
      temporary,
      locked: false
    }),
    temporary,
    (t) => t.kind === 'tool' && t.tool === 'notification-test' && !t.temporary
  )
}

export function promoteTab(projectId: string, tabId: TabId): void {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const tab = findTab(ws, tabId)
  if (!tab || tab.kind === 'session' || tab.kind === 'launcher' || tab.kind === 'task' || !tab.temporary) return

  ws.tabs = ws.tabs.map((candidate) => (candidate.id === tabId ? { ...candidate, temporary: false } : candidate))
  commitWorkspace(ws)
}

export function promoteTemporaryTab(tabId: TabId) {
  if (!activeProjectId) return
  promoteTab(activeProjectId, tabId)
}

export function promoteTabWhenReady(projectId: string, pendingTabId: TabId | Promise<TabId> | null | undefined): void {
  if (!pendingTabId) return
  void Promise.resolve(pendingTabId)
    .then((tabId) => promoteTab(projectId, tabId))
    .catch(() => {})
}

/**
 * Promote the focused pane's active tab to persistent, if it's a
 * temporary content tab. Sessions and launcher tabs fall through
 * unchanged, so callers can invoke this regardless of focused kind.
 */
export function promoteFocusedTab(projectId: string): void {
  const tab = getFocusedTab(projectId)
  if (!tab) return
  promoteTab(projectId, tab.id)
}

export function closeTab(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const tab = findTab(ws, tabId)
  if (!tab) return
  if (tab.locked) return

  // Push a restorable snapshot before teardown. `tabToPersisted` returns
  // null for tabs that can't be meaningfully reopened (unsaved new-file
  // buffers, notification tool tabs); those simply don't enter the
  // reopen stack.
  const snapshot = tabToPersisted(tab)
  if (snapshot) {
    const list = closedTabs.get(projectId) ?? []
    setClosedTabsForProject(projectId, [snapshot, ...list].slice(0, MAX_CLOSED_TABS))
  }

  // Dispose the frontend terminal manager. The PTY should already be
  // stopped by the caller (PaneTabBar.handleTabClose), but dispose
  // ensures the xterm instance and channels are cleaned up.
  if (tab.kind === 'session') {
    sessionRegistry.dispose(tab.sessionId)
  } else if (tab.kind === 'task') {
    taskRegistry.dispose(tab.runId)
  }

  // Remove from pane
  for (const pane of ws.panes) {
    if (pane.tabs.includes(tabId)) {
      removeTabFromPane(pane, tabId)
      break
    }
  }

  // Remove from workspace tabs. Collapse-empty also commits, so skip
  // the redundant commit here; collapsePaneIfEmpty will pick up the
  // mutated `ws` and persist a single coherent snapshot.
  ws.tabs = ws.tabs.filter((t) => t.id !== tabId)

  // Collapse splits that are now empty; otherwise closing the last tab
  // in a split pane leaves an orphan launcher surface the user has
  // to close manually.
  collapsePaneIfEmpty(projectId)
}

/**
 * Pop the most recently closed tab off the per-project stack and re-add
 * it via the normal add* function for that kind. Returns the new tab id,
 * or null when the stack is empty. Mirrors VSCode's Ctrl+Shift+T.
 */
export async function reopenLastClosedTab(projectId: string): Promise<TabId | null> {
  await restoreWorkspaceFromDisk(projectId)

  while (true) {
    const head = popClosedTabForProject(projectId)
    if (!head) return null

    switch (head.kind) {
      case 'session':
        try {
          await backend.sessions.get(head.sessionId)
        } catch {
          continue
        }
        return addSessionTab(projectId, head.sessionId, head.title, head.providerId)
      case 'text':
        return head.gitRef
          ? addReadonlyTextTab(projectId, head.filePath, head.gitRef, head.refLabel ?? head.gitRef, false)
          : addTextTab(projectId, head.filePath, false)
      case 'diff':
        switch (head.source.kind) {
          case 'working':
            return addChangesTab(projectId, head.source.staged, head.source.scopePath ?? null, head.initialFile, false)
          case 'commit':
            return addCommitTab(
              projectId,
              head.source.commitHash,
              head.source.shortHash,
              head.source.message,
              head.initialFile,
              false
            )
          case 'stash':
            return addStashTab(projectId, head.source.stashIndex, head.source.message, head.initialFile, false)
          default: {
            const _exhaustive: never = head.source
            return _exhaustive
          }
        }
      case 'tool':
        return null
      default: {
        const _exhaustive: never = head
        return _exhaustive
      }
    }
  }
}

export function hasClosedTabs(projectId: string): boolean {
  return (closedTabs.get(projectId)?.length ?? 0) > 0
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

// PANE FOCUS //
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

// SPLIT PANE OPERATIONS //
/**
 * Split the current focused pane in a direction.
 * Returns the slot of the new pane, or null if already at max.
 *
 * single → horizontal or vertical
 * horizontal/vertical → quad
 * quad → null (max 4 panes)
 */
export function canSplitPane(projectId: string, paneSlot: PaneSlot, direction: SplitDirection): boolean {
  const ws = getWorkspace(projectId)
  if (!ws) return false

  if (ws.splitMode === 'single') return true
  if (ws.splitMode === 'horizontal') {
    return (direction === 'up' || direction === 'down') && (paneSlot === 'left' || paneSlot === 'right')
  }
  if (ws.splitMode === 'vertical') {
    return (direction === 'left' || direction === 'right') && (paneSlot === 'left' || paneSlot === 'right')
  }
  if (ws.splitMode !== 'quad' || ws.panes.length >= 4) return false

  if (ws.quadLayout === 'top') {
    return paneSlot === 'top-left' && (direction === 'left' || direction === 'right')
  }
  if (ws.quadLayout === 'bottom') {
    return paneSlot === 'bottom-left' && (direction === 'left' || direction === 'right')
  }
  if (ws.quadLayout === 'left') {
    return paneSlot === 'top-left' && (direction === 'up' || direction === 'down')
  }
  if (ws.quadLayout === 'right') {
    return paneSlot === 'top-right' && (direction === 'up' || direction === 'down')
  }

  return false
}

export function splitPaneAt(projectId: string, paneSlot: PaneSlot, direction: SplitDirection): PaneSlot | null {
  const ws = getWorkspace(projectId)
  if (!ws) return null
  if (!canSplitPane(projectId, paneSlot, direction)) return null

  let newSlot: PaneSlot

  if (ws.splitMode === 'single') {
    const sole = ws.panes[0]
    if (!sole) return null

    if (direction === 'left' || direction === 'right') {
      ws.splitMode = 'horizontal'
      ws.quadLayout = null
      if (direction === 'right') {
        sole.slot = 'left'
        ws.panes = [sole, createPane('right')]
        newSlot = 'right'
      } else {
        sole.slot = 'right'
        ws.panes = [createPane('left'), sole]
        newSlot = 'left'
      }
    } else {
      ws.splitMode = 'vertical'
      ws.quadLayout = null
      if (direction === 'down') {
        sole.slot = 'left'
        ws.panes = [sole, createPane('right')]
        newSlot = 'right'
      } else {
        sole.slot = 'right'
        ws.panes = [createPane('left'), sole]
        newSlot = 'left'
      }
    }

    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.splitMode === 'horizontal') {
    const left = ws.panes.find((pane) => pane.slot === 'left')
    const right = ws.panes.find((pane) => pane.slot === 'right')
    if (!left || !right) return null

    if (paneSlot === 'left') {
      right.slot = 'top-right'
      ws.splitMode = 'quad'
      ws.quadLayout = 'right'

      if (direction === 'down') {
        left.slot = 'top-left'
        ws.panes = [left, right, createPane('bottom-left')]
        newSlot = 'bottom-left'
      } else {
        left.slot = 'bottom-left'
        ws.panes = [createPane('top-left'), right, left]
        newSlot = 'top-left'
      }
    } else {
      left.slot = 'top-left'
      ws.splitMode = 'quad'
      ws.quadLayout = 'left'

      if (direction === 'down') {
        right.slot = 'top-right'
        ws.panes = [left, right, createPane('bottom-right')]
        newSlot = 'bottom-right'
      } else {
        right.slot = 'bottom-right'
        ws.panes = [left, createPane('top-right'), right]
        newSlot = 'top-right'
      }
    }

    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.splitMode === 'vertical') {
    const top = ws.panes.find((pane) => pane.slot === 'left')
    const bottom = ws.panes.find((pane) => pane.slot === 'right')
    if (!top || !bottom) return null

    if (paneSlot === 'left') {
      bottom.slot = 'bottom-left'
      ws.splitMode = 'quad'
      ws.quadLayout = 'bottom'

      if (direction === 'right') {
        top.slot = 'top-left'
        ws.panes = [top, bottom, createPane('top-right')]
        newSlot = 'top-right'
      } else {
        top.slot = 'top-right'
        ws.panes = [createPane('top-left'), top, bottom]
        newSlot = 'top-left'
      }
    } else {
      top.slot = 'top-left'
      ws.splitMode = 'quad'
      ws.quadLayout = 'top'

      if (direction === 'right') {
        bottom.slot = 'bottom-left'
        ws.panes = [top, bottom, createPane('bottom-right')]
        newSlot = 'bottom-right'
      } else {
        bottom.slot = 'bottom-right'
        ws.panes = [top, createPane('bottom-left'), bottom]
        newSlot = 'bottom-left'
      }
    }

    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'top' && paneSlot === 'top-left' && (direction === 'left' || direction === 'right')) {
    if (direction === 'right') {
      ws.panes = [...ws.panes, createPane('top-right')]
      newSlot = 'top-right'
    } else {
      const top = ws.panes.find((pane) => pane.slot === 'top-left')
      if (!top) return null
      top.slot = 'top-right'
      ws.panes = [...ws.panes.filter((pane) => pane !== top), createPane('top-left'), top]
      newSlot = 'top-left'
    }

    ws.quadLayout = null
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'bottom' && paneSlot === 'bottom-left' && (direction === 'left' || direction === 'right')) {
    if (direction === 'right') {
      ws.panes = [...ws.panes, createPane('bottom-right')]
      newSlot = 'bottom-right'
    } else {
      const bottom = ws.panes.find((pane) => pane.slot === 'bottom-left')
      if (!bottom) return null
      bottom.slot = 'bottom-right'
      ws.panes = [...ws.panes.filter((pane) => pane !== bottom), createPane('bottom-left'), bottom]
      newSlot = 'bottom-left'
    }

    ws.quadLayout = null
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'left' && paneSlot === 'top-left' && (direction === 'up' || direction === 'down')) {
    if (direction === 'down') {
      ws.panes = [...ws.panes, createPane('bottom-left')]
      newSlot = 'bottom-left'
    } else {
      const left = ws.panes.find((pane) => pane.slot === 'top-left')
      if (!left) return null
      left.slot = 'bottom-left'
      ws.panes = [...ws.panes.filter((pane) => pane !== left), createPane('top-left'), left]
      newSlot = 'top-left'
    }

    ws.quadLayout = null
    ws.focusedPaneSlot = newSlot
    commitWorkspace(ws)
    return newSlot
  }

  if (ws.quadLayout === 'right' && paneSlot === 'top-right' && (direction === 'up' || direction === 'down')) {
    if (direction === 'down') {
      ws.panes = [...ws.panes, createPane('bottom-right')]
      newSlot = 'bottom-right'
    } else {
      const right = ws.panes.find((pane) => pane.slot === 'top-right')
      if (!right) return null
      right.slot = 'bottom-right'
      ws.panes = [...ws.panes.filter((pane) => pane !== right), createPane('top-right'), right]
      newSlot = 'top-right'
    }

    ws.quadLayout = null
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
    // Closing the last tab leaves the user with no surface to act on.
    // Reset to a single pane and seed a launcher tab so the picker is
    // reachable as a real tab (matching fresh-project behaviour).
    resetToSinglePane(ws)
    seedLauncherTab(ws, ws.panes[0])
    commitWorkspace(ws)
    return
  }

  // Remove empty panes (keep at least one)
  const nonEmpty = ws.panes.filter((p) => p.tabs.length > 0)
  if (nonEmpty.length === 0) {
    // Tabs exist but no pane owns them; orphan state recovery.
    // resetToSinglePane re-homes every tab, so we don't need to seed.
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
 * tabs whose sessions were renamed. The auto-create heuristic; adding
 * tabs for any active session; only fires when no persisted workspace
 * exists yet (legacy bootstrap), so we never silently re-spawn dead
 * tab layouts on top of a restored workspace.
 */
export function syncSessionTabs(projectId: string, sessions: Session[]) {
  const ws = ensureWorkspace(projectId)
  const nextSessionIds = new Set(sessions.map((session) => session.id))
  pruneClosedSessionTabs(projectId, nextSessionIds)
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
  // this project; whether the blob existed or not; restored tabs are
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

  // Single commit; collapsePaneIfEmpty also commits, so only call it
  // when there are genuinely empty panes to avoid a redundant clone.
  const hasEmptyPanes = ws.panes.some((pane) => pane.tabs.length === 0)
  commitWorkspace(ws)
  if (hasEmptyPanes) {
    collapsePaneIfEmpty(projectId)
  }
}

export function toggleTabLocked(projectId: string, tabId: TabId) {
  const ws = getWorkspace(projectId)
  if (!ws) return

  const tab = findTab(ws, tabId)
  if (!tab) return
  // Defense: the UI already hides the Lock menu item for non-lockable
  // kinds, but guarding here means a stale persisted blob or a future
  // caller can't sneak a locked state onto a tab that the rest of the
  // code assumes is always unlocked.
  if (!canLockTab(tab)) return

  ws.tabs = ws.tabs.map((t) => (t.id === tabId ? { ...t, locked: !t.locked } : t))
  commitWorkspace(ws)
}

// DERIVED SESSION IDENTITY //
/** The active session is the session tab that's active in the focused pane. */
export function getActiveSessionId(): string | null {
  const ws = activeProjectId ? getWorkspace(activeProjectId) : undefined
  if (!ws) return null

  const pane = ws.panes.find((p) => p.slot === ws.focusedPaneSlot) ?? ws.panes[0]
  if (!pane?.activeTabId) return null

  const tab = ws.tabs.find((t) => t.id === pane.activeTabId)
  return tab?.kind === 'session' ? tab.sessionId : null
}
