// Workbench state — the single global tab list.
//
// Every tab carries its own `folderPath`; the active tab decides which
// folder the sidebar, status bar, and folder-scoped commands operate on.
// There is no project lifecycle: opening a folder either focuses one of
// its tabs or seeds a new tab page for it.

import { SvelteSet } from 'svelte/reactivity'
import { backend } from '$lib/api/backend'
import { platform, requireNative } from '$lib/platform'
import { releaseFolder } from '$lib/features/folders/lifecycle'
import { filterExistingFolders, getRecentFolders, pushRecentFolder } from '$lib/features/folders/state.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
import * as taskRegistry from '$lib/features/tasks/taskRegistry'
import type {
  DiffSource,
  DiffTab,
  EpicTab,
  IssueTab,
  NewTab,
  PersistedTab,
  SessionStatus,
  SessionTab,
  Tab,
  TabId,
  TaskRunStatus,
  TaskTab,
  TextTab,
  ToolTab,
  Workbench
} from '$lib/features/workbench/model'
import { canLockTab, tabServer } from '$lib/features/workbench/model'
import { getGroupRefs, isTabInert, restoreGroups, syncGroups } from './groups.svelte'
import {
  loadPersistedWorkbench,
  flushWorkbench,
  persistedToTab,
  schedulePersistWorkbench,
  serializeWorkbench,
  tabToPersisted
} from '$lib/features/workbench/persistence'
import { clampTabInsertionToGroup, compactGroupTabs, computeTabInsertion, reorderWithinGroup } from './tabInsertion'
import {
  ensureTextFileSyncListeners,
  openTextFile,
  revealTextTab,
  type TextRevealTarget
} from '$lib/features/workbench/surfaces/text/service.svelte'
import { basename, resolveProjectFile, splitRemotePath } from '$lib/utils/paths'

export type {
  DiffSource,
  DiffTab,
  EpicTab,
  IssueTab,
  NewTab,
  PersistedTab,
  SessionStatus,
  SessionTab,
  Tab,
  TabId,
  TaskRunStatus,
  TaskTab,
  TextTab,
  ToolTab,
  Workbench
}
export { canLockTab }

// MODULE STATE //
let workbench = $state<Workbench>({ tabs: [], activeTabId: null })
let workbenchId = 'main'

export function setWorkbenchId(id: string): void {
  workbenchId = id
}

export function getWorkbenchId(): string {
  return workbenchId
}

// LIFO stack of recently closed tabs for Ctrl+Shift+T. In-memory only: a
// fresh launch has nothing to reopen beyond what restore hydrates.
const MAX_CLOSED_TABS = 20
let closedTabs = $state<PersistedTab[]>([])

// Most recently active tab per folder, so "Open Folder" on an already
// open folder lands where the user last was rather than on its last tab.
const lastActiveByFolder = new Map<string, TabId>()

// Until restore has consulted disk, commits must not persist or the
// seeded empty state would clobber the saved blob.
let restored = false
let pendingFocusTabId: TabId | null = null
const transferringTabIds = new SvelteSet<TabId>()
const stagedTabIds = new SvelteSet<TabId>()
const folderOps = new Map<string, Promise<void>>()

function queueFolderOp(folderPath: string, op: () => Promise<void>): Promise<void> {
  const prev = folderOps.get(folderPath) ?? Promise.resolve()
  const next = prev.catch(() => {}).then(op)
  folderOps.set(folderPath, next)
  void next
    .finally(() => {
      if (folderOps.get(folderPath) === next) folderOps.delete(folderPath)
    })
    .catch(() => {})
  return next
}

export function awaitFolderOps(): Promise<void> {
  return Promise.all(folderOps.values()).then(() => {})
}

export function setTabTransferring(tabId: TabId, on: boolean): void {
  if (on) transferringTabIds.add(tabId)
  else transferringTabIds.delete(tabId)
}

export function isTabTransferring(tabId: TabId): boolean {
  return transferringTabIds.has(tabId)
}

// Monotonic counter for "Untitled-N" labels on new untitled text tabs.
let untitledCounter = 0

// HELPERS //
export function generateTabId(): TabId {
  return `tab-${crypto.randomUUID()}`
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

function findTab(tabId: TabId): Tab | undefined {
  return workbench.tabs.find((t) => t.id === tabId)
}

/**
 * Single choke point for layout mutation: reassigns the workbench so
 * `$derived` consumers see a new reference, updates backend folder
 * ownership, then schedules a debounced persist.
 */
function commit(next: { tabs?: Tab[]; activeTabId?: TabId | null } = {}, persist = true): void {
  const previousFolders = new Set(workbench.tabs.map((tab) => tab.folderPath))
  const tabs =
    next.tabs && platform.capabilities.remoteHosts ? compactGroupTabs(next.tabs) : (next.tabs ?? workbench.tabs)
  const nextFolders = new Set(tabs.map((tab) => tab.folderPath))
  const activeTabId = next.activeTabId === undefined ? workbench.activeTabId : next.activeTabId
  workbench = { tabs: [...tabs], activeTabId }
  const active = activeTabId ? tabs.find((t) => t.id === activeTabId) : undefined
  if (active) lastActiveByFolder.set(active.folderPath, active.id)
  for (const folderPath of nextFolders) {
    if (!previousFolders.has(folderPath)) {
      void queueFolderOp(folderPath, () =>
        backend.folders.claim(folderPath).catch((e) => console.warn('Folder claim failed:', e))
      )
    }
  }
  for (const folderPath of previousFolders) {
    if (!nextFolders.has(folderPath)) void queueFolderOp(folderPath, () => releaseFolder(folderPath))
  }
  if (persist) persistWorkbench()
  if (restored) syncGroups()
}

export function persistWorkbench(): void {
  if (!restored) return
  schedulePersistWorkbench(workbenchId, () => {
    const tabs = workbench.tabs.filter((tab) => !stagedTabIds.has(tab.id))
    const local = platform.capabilities.remoteHosts ? tabs.filter((tab) => !tabServer(tab)) : tabs
    const activeTabId = workbench.activeTabId
    const active = tabs.find((tab) => tab.id === activeTabId)
    const activeServer = active ? tabServer(active) : null
    const groupTabs = activeServer ? tabs.filter((tab) => tabServer(tab) === activeServer) : []
    return {
      ...serializeWorkbench({
        tabs: local,
        activeTabId: activeTabId != null && stagedTabIds.has(activeTabId) ? (local[0]?.id ?? null) : activeTabId
      }),
      ...(platform.capabilities.remoteHosts
        ? {
            groups: getGroupRefs(),
            ...(activeServer
              ? { activeGroup: { server: activeServer, index: groupTabs.findIndex((tab) => tab.id === activeTabId) } }
              : {})
          }
        : {})
    }
  })
}

/**
 * Insert and activate a tab.
 * - New tabs are transient: the first real tab opened replaces that
 *   folder's new tab page in-place.
 * - Same-folder tabs open directly next to the active tab to keep folder tabs grouped.
 * - Other-folder tabs open after that folder's existing tabs.
 */
function insertTab(tab: Tab): TabId {
  if (isTabInert(tab)) throw new Error('This server workbench is not controlled by this window')
  const placement = computeTabInsertion(workbench.tabs, workbench.activeTabId, tab)
  const tabs = [...workbench.tabs]
  if ('replaceIndex' in placement) {
    tabs[placement.replaceIndex] = tab
  } else {
    tabs.splice(clampTabInsertionToGroup(tabs, tab, placement.insertIndex), 0, tab)
  }
  commit({ tabs, activeTabId: tab.id })
  return tab.id
}

function updateTab(tabId: TabId, update: (tab: Tab) => Tab): void {
  const index = workbench.tabs.findIndex((tab) => tab.id === tabId)
  if (index === -1) return

  const updated = update(workbench.tabs[index])
  if (updated === workbench.tabs[index]) return

  const tabs = [...workbench.tabs]
  tabs[index] = updated
  commit({ tabs })
}

function pushClosedTab(snapshot: PersistedTab): void {
  closedTabs = [snapshot, ...closedTabs].slice(0, MAX_CLOSED_TABS)
}

// READS //
export function getTabs(): Tab[] {
  return workbench.tabs
}

export function getActiveTab(): Tab | null {
  return workbench.activeTabId ? (findTab(workbench.activeTabId) ?? null) : null
}

export function getActiveTabId(): TabId | null {
  return workbench.activeTabId
}

export function getActiveFolderPath(): string | null {
  return getActiveTab()?.folderPath ?? null
}

/** The active tab's id when it is a session tab. */
export function getActiveSessionTabId(): TabId | null {
  const tab = getActiveTab()
  return tab?.kind === 'session' ? tab.id : null
}

export function hasClosedTabs(): boolean {
  return closedTabs.length > 0
}

/** Recent folders without an open tab; feeds the "Open Recent" menus. */
export function getRecentUnopenedFolders(): string[] {
  const open = new Set(workbench.tabs.map((t) => t.folderPath))
  return getRecentFolders().flatMap(({ path }) => (open.has(path) ? [] : [path]))
}

/** Find an existing live task tab by its source task id (singleton rerun). */
export function findTaskTabByTaskId(folderPath: string, taskId: string): TaskTab | null {
  return (
    workbench.tabs.find(
      (t): t is TaskTab =>
        t.kind === 'task' &&
        t.folderPath === folderPath &&
        t.taskId === taskId &&
        t.status !== 'exited' &&
        t.status !== 'failed'
    ) ?? null
  )
}

// ACTIVATION / ORDER //
export function setActiveTab(tabId: TabId): void {
  if (workbench.activeTabId === tabId || !findTab(tabId)) return
  commit({ activeTabId: tabId })
}

export function requestFocusTab(tabId: TabId, reveal?: TextRevealTarget | null): void {
  if (findTab(tabId)) {
    setActiveTab(tabId)
    if (reveal) revealTextTab(tabId, reveal)
  } else if (!restored) {
    pendingFocusTabId = tabId
  }
}

export function reorderTab(fromIndex: number, toIndex: number): void {
  const tabs = workbench.tabs
  if (fromIndex < 0 || toIndex < 0 || fromIndex >= tabs.length || toIndex >= tabs.length) return
  const next = reorderWithinGroup(tabs, fromIndex, toIndex)
  if (next) commit({ tabs: next })
}
/** Stage an imported tab without persisting it before the broker commits. */
export function stageTransferredTab(tab: Tab, targetIndex: number): void {
  if (findTab(tab.id)) throw new Error(`Tab ${tab.id} already exists in target window`)
  transferringTabIds.add(tab.id)
  stagedTabIds.add(tab.id)
  const tabs = [...workbench.tabs]
  tabs.splice(clampTabInsertionToGroup(tabs, tab, Math.max(0, Math.min(targetIndex, tabs.length))), 0, tab)
  commit({ tabs, activeTabId: tab.id }, false)
}

/** Remove a transferred tab without close prompts, process teardown, or file-claim release. */
export function removeTransferredTab(tabId: TabId, persist = true): void {
  transferringTabIds.delete(tabId)
  stagedTabIds.delete(tabId)
  const index = workbench.tabs.findIndex((tab) => tab.id === tabId)
  if (index < 0) return
  const tab = workbench.tabs[index]
  const tabs = workbench.tabs.filter((candidate) => candidate.id !== tabId)
  const activeTabId =
    workbench.activeTabId === tabId ? ((tabs[index] ?? tabs[index - 1])?.id ?? null) : workbench.activeTabId
  if (lastActiveByFolder.get(tab.folderPath) === tabId) lastActiveByFolder.delete(tab.folderPath)
  commit({ tabs, activeTabId }, persist)
}

/** Replace one server's block without closing its runs (controller handoff). */
export function replaceGroupTabs(server: string, replacement: Tab[], atIndex?: number, activeIndex = 0): void {
  const first = workbench.tabs.findIndex((tab) => tabServer(tab) === server)
  const existing = workbench.tabs.filter((tab) => tabServer(tab) === server)
  const tabs = workbench.tabs.filter((tab) => tabServer(tab) !== server)
  const index = Math.max(0, Math.min(atIndex ?? (first < 0 ? tabs.length : first), tabs.length))
  tabs.splice(index, 0, ...replacement)
  const wasActive = existing.some((tab) => tab.id === workbench.activeTabId)
  const activeTabId =
    wasActive || (!workbench.activeTabId && replacement.length)
      ? (replacement[activeIndex]?.id ?? replacement[0]?.id ?? tabs[index]?.id ?? tabs[index - 1]?.id ?? null)
      : workbench.activeTabId
  commit({ tabs, activeTabId })
}

export function finalizeTransferredTab(tabId: TabId): void {
  transferringTabIds.delete(tabId)
  stagedTabIds.delete(tabId)
  persistWorkbench()
}

export function toggleTabLocked(tabId: TabId): void {
  // The UI hides Lock for non-lockable kinds; guarding here keeps a stale
  // blob or future caller from locking a tab the rest of the code assumes
  // is always unlocked.
  updateTab(tabId, (t) => (canLockTab(t) ? { ...t, locked: !t.locked } : t))
}

// FOLDER ENTRY //
/**
 * Open a folder: canonicalize, remember it, then focus its most recently
 * active tab or seed a new tab page when the folder has none.
 */
export async function openFolder(path: string): Promise<void> {
  let folderPath: string
  try {
    folderPath = (await backend.folders.resolve(path)).path
  } catch (error) {
    notify.error('Open folder failed', getErrorMessage(error))
    return
  }
  pushRecentFolder(folderPath)

  const remembered = lastActiveByFolder.get(folderPath)
  const existing =
    (remembered && findTab(remembered)?.folderPath === folderPath ? remembered : undefined) ??
    workbench.tabs.findLast((t) => t.folderPath === folderPath)?.id
  if (existing) {
    setActiveTab(existing)
    return
  }
  openNewTab(folderPath)
}

/** Turn the originating new tab page into another folder without adding a tab. */
export async function retargetNewTab(tabId: TabId, path: string): Promise<void> {
  let folderPath: string
  try {
    folderPath = (await backend.folders.resolve(path)).path
  } catch (error) {
    notify.error('Open folder failed', getErrorMessage(error))
    return
  }
  pushRecentFolder(folderPath)

  const tab = findTab(tabId)
  if (tab?.kind !== 'new-tab') {
    await openFolder(folderPath)
    return
  }
  if (tab.folderPath === folderPath) {
    setActiveTab(tabId)
    return
  }
  const replacement: NewTab = { ...tab, folderPath }
  if (isTabInert(tab) || isTabInert(replacement)) {
    notify.error('Open folder failed', 'This server workbench is not controlled by this window')
    return
  }
  const tabs = workbench.tabs
    .filter(
      (candidate) => candidate.id === tabId || candidate.kind !== 'new-tab' || candidate.folderPath !== folderPath
    )
    .map((candidate) => (candidate.id === tabId ? replacement : candidate))
  commit({ tabs, activeTabId: tabId })
}

// RESTORE //
/**
 * Hydrate the persisted tab list. Tabs whose folder no longer resolves are
 * dropped; session tabs come back dormant and start on first activation.
 */
export async function restoreWorkbench(workbenchId: string): Promise<void> {
  try {
    await ensureTextFileSyncListeners()
    const persisted = await loadPersistedWorkbench(workbenchId)
    if (persisted) {
      const folders = await filterExistingFolders([
        ...new Set(persisted.tabs.filter((t) => !splitRemotePath(t.folderPath)).map((t) => t.folderPath))
      ])
      const alive = new Set(folders)

      // Claims run before commit, so a redirect that lands mid-restore is parked in pendingFocusTabId and applied at commit.
      // Keep original indices for nearest-neighbour fallback after drops.
      const candidates: Array<{ tab: Tab; index: number }> = []
      for (const [index, entry] of persisted.tabs.entries()) {
        if (alive.has(entry.folderPath) || (platform.capabilities.remoteHosts && splitRemotePath(entry.folderPath)))
          candidates.push({ tab: persistedToTab(entry, generateTabId()), index })
      }

      const results = await Promise.all(
        candidates.map(async (c) => ({
          c,
          redirect:
            platform.capabilities.fileClaims &&
            c.tab.kind === 'text' &&
            c.tab.filePath != null &&
            !c.tab.gitRef &&
            (await requireNative().files.claimFile(resolveProjectFile(c.tab.folderPath, c.tab.filePath), c.tab.id))
              .status === 'redirect'
        }))
      )
      const survivors = results
        .filter(({ c, redirect }) => {
          if (redirect && c.tab.kind === 'text') {
            notify.warning('Duplicate file tab dropped', `File ${c.tab.filePath} is open in another window.`)
          }
          return !redirect
        })
        .map(({ c }) => c)

      const active =
        survivors.find((candidate) => candidate.tab.id === pendingFocusTabId) ??
        survivors.find((candidate) => candidate.index >= persisted.activeTabIndex) ??
        survivors.findLast((candidate) => candidate.index < persisted.activeTabIndex)
      pendingFocusTabId = null
      commit({ tabs: survivors.map((candidate) => candidate.tab), activeTabId: active?.tab.id ?? null })
      await awaitFolderOps()
      if (platform.capabilities.remoteHosts) {
        await restoreGroups(persisted.groups ?? [])
        const activeGroup = persisted.activeGroup
        if (activeGroup) {
          const tabs = workbench.tabs.filter((tab) => tabServer(tab) === activeGroup.server)
          const tab = tabs[activeGroup.index]
          if (tab) commit({ activeTabId: tab.id })
        }
      }
    }
  } catch (error) {
    console.warn('Workbench restore failed, starting empty:', error)
  } finally {
    restored = true
    syncGroups()
    persistWorkbench()
  }
}

// SESSION TABS //
export interface SessionTabInit {
  providerId: string
  title: string
  resumeToken: string | null
}

/** Add a dormant session tab; the mounted surface spawns its process. */
export function addSessionTab(folderPath: string, init: SessionTabInit): TabId {
  return insertTab({
    kind: 'session',
    id: generateTabId(),
    folderPath,
    title: init.title,
    providerId: init.providerId,
    runId: null,
    resumeToken: init.resumeToken,
    status: 'dormant',
    locked: false
  })
}

export function setSessionTabStatus(tabId: TabId, status: SessionStatus): void {
  updateTab(tabId, (t) => (t.kind === 'session' && t.status !== status ? { ...t, status } : t))
}

export function setSessionTabTitle(tabId: TabId, title: string): void {
  if (!title) return
  updateTab(tabId, (t) => (t.kind === 'session' && t.title !== title ? { ...t, title } : t))
}
export function setSessionTabRunId(tabId: TabId, runId: string | null): void {
  updateTab(tabId, (t) => (t.kind === 'session' && t.runId !== runId ? { ...t, runId } : t))
}
/**
 * Persist a newly minted run id before asking the backend to start it.
 * Durability is best effort: the failed snapshot is requeued by
 * `flushWorkbench`, and a lost run id only costs reattach after a restart.
 */
export async function persistSessionTabRunId(tabId: TabId, runId: string): Promise<void> {
  setSessionTabRunId(tabId, runId)
  await flushWorkbench(workbenchId).catch((error) => console.warn('Failed to persist session run id:', error))
}

export function setSessionTabResumeToken(tabId: TabId, resumeToken: string | null): void {
  updateTab(tabId, (t) => (t.kind === 'session' && t.resumeToken !== resumeToken ? { ...t, resumeToken } : t))
}

/** Drop the tab's token only if it still equals `expectedToken`; a token bound meanwhile wins. */
export function clearSessionTabResumeToken(tabId: TabId, expectedToken: string): void {
  updateTab(tabId, (t) => (t.kind === 'session' && t.resumeToken === expectedToken ? { ...t, resumeToken: null } : t))
}

// TASK TABS //
export interface TaskTabInit {
  runId: string
  taskId: string
  activeFilePath: string | null
  label: string
  icon: string | null
  group: string | null
}

/**
 * Add a new task tab. Always creates a fresh tab; callers that want
 * singleton focus-on-rerun use `findTaskTabByTaskId` first.
 */
export function addTaskTab(folderPath: string, init: TaskTabInit): TabId {
  return insertTab({
    kind: 'task',
    id: generateTabId(),
    folderPath,
    runId: init.runId,
    attachOnly: false,
    taskId: init.taskId,
    activeFilePath: init.activeFilePath,
    label: init.label,
    icon: init.icon,
    group: init.group,
    status: 'starting',
    exitCode: null,
    locked: false
  })
}

export function setTaskTabStatus(tabId: TabId, status: TaskRunStatus, exitCode: number | null = null): void {
  updateTab(tabId, (t) =>
    t.kind === 'task' && (t.status !== status || t.exitCode !== exitCode) ? { ...t, status, exitCode } : t
  )
}

/**
 * Rebind a singleton task tab to a fresh runId for restart. Resets
 * lifecycle status and caches the latest label/icon in case the task
 * definition changed since the tab was created.
 */
export function resetTaskTabForRestart(
  tabId: TabId,
  nextRunId: string,
  latest: {
    activeFilePath: string | null
    label: string
    icon: string | null
    group: string | null
  }
): void {
  updateTab(tabId, (t) =>
    t.kind === 'task'
      ? {
          ...t,
          runId: nextRunId,
          attachOnly: false,
          activeFilePath: latest.activeFilePath,
          label: latest.label,
          icon: latest.icon,
          group: latest.group,
          status: 'starting',
          exitCode: null
        }
      : t
  )
  setActiveTab(tabId)
}

// CONTENT TABS //
/** Compare the content-bearing fields of two tabs (ignoring id/locked). */
function tabDataChanged(a: Tab, b: Tab): boolean {
  if (a.kind !== b.kind || a.folderPath !== b.folderPath) return true
  switch (a.kind) {
    case 'diff':
      return b.kind !== 'diff' || !diffSourcesEqual(a.source, b.source) || a.initialFile !== b.initialFile
    case 'text':
      return b.kind !== 'text' || a.filePath !== b.filePath || a.gitRef !== b.gitRef
    case 'tool':
      return b.kind !== 'tool' || a.tool !== b.tool || a.label !== b.label
    case 'issue':
      return b.kind !== 'issue' || a.issueId !== b.issueId
    case 'epic':
      return b.kind !== 'epic' || a.epicId !== b.epicId
    default:
      return true
  }
}

/**
 * Generic content-tab helper. Handles the 3-phase pattern:
 * 1. Replace the single temporary tab of this kind (global, any folder)
 * 2. Reuse an existing persistent tab (optional update via `onReuse`)
 * 3. Create a new tab
 */
function addContentTab(
  kind: Tab['kind'],
  makeTab: (id: TabId) => Tab,
  temporary: boolean,
  matchPersistent?: (t: Tab) => boolean,
  onReuse?: (existing: Tab) => Tab,
  tabId?: TabId
): TabId {
  if (temporary) {
    const existingTemp = tabId
      ? workbench.tabs.find(
          (t) => t.id === tabId && t.kind === kind && t.kind !== 'session' && t.kind !== 'task' && t.temporary
        )
      : workbench.tabs.find((t) => t.kind === kind && t.kind !== 'session' && t.kind !== 'task' && t.temporary)
    if (existingTemp) {
      const newTab = makeTab(existingTemp.id)
      if (isTabInert(newTab)) throw new Error('This server workbench is not controlled by this window')
      if (
        existingTemp.kind === 'text' &&
        existingTemp.filePath != null &&
        !existingTemp.gitRef &&
        (newTab.kind !== 'text' ||
          newTab.folderPath !== existingTemp.folderPath ||
          newTab.filePath !== existingTemp.filePath ||
          newTab.gitRef)
      ) {
        if (platform.capabilities.fileClaims)
          void requireNative().files.releaseFile(resolveProjectFile(existingTemp.folderPath, existingTemp.filePath))
      }
      // Skip mutation when tab data hasn't changed (second click of a
      // double-click on the same file).
      if (!tabDataChanged(existingTemp, newTab)) {
        setActiveTab(existingTemp.id)
        return existingTemp.id
      }
      if (existingTemp.folderPath === newTab.folderPath) {
        commit({
          tabs: workbench.tabs.map((t) => (t.id === existingTemp.id ? newTab : t)),
          activeTabId: newTab.id
        })
        return newTab.id
      }

      const remainingTabs = workbench.tabs.filter((t) => t.id !== existingTemp.id)
      const placement = computeTabInsertion(remainingTabs, workbench.activeTabId, newTab)
      if ('replaceIndex' in placement) {
        remainingTabs[placement.replaceIndex] = newTab
      } else {
        remainingTabs.splice(clampTabInsertionToGroup(remainingTabs, newTab, placement.insertIndex), 0, newTab)
      }
      commit({ tabs: remainingTabs, activeTabId: newTab.id })
      return newTab.id
    }
  }

  if (matchPersistent) {
    const existing = workbench.tabs.find((t) => matchPersistent(t))
    if (existing) {
      const updated = onReuse ? onReuse(existing) : existing
      commit({
        tabs: updated === existing ? workbench.tabs : workbench.tabs.map((t) => (t.id === existing.id ? updated : t)),
        activeTabId: existing.id
      })
      return existing.id
    }
  }

  return insertTab(makeTab(tabId ?? generateTabId()))
}

export function addCommitTab(
  folderPath: string,
  commitHash: string,
  shortHash: string,
  message: string,
  initialFile: string | null = null,
  temporary = true
): TabId {
  return addContentTab(
    'diff',
    (id): DiffTab => ({
      kind: 'diff',
      id,
      folderPath,
      source: { kind: 'commit', commitHash, shortHash, message },
      initialFile,
      temporary,
      locked: false
    }),
    temporary,
    (t) =>
      t.kind === 'diff' &&
      t.folderPath === folderPath &&
      t.source.kind === 'commit' &&
      t.source.commitHash === commitHash &&
      !t.temporary,
    (t) => (t.kind === 'diff' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addChangesTab(
  folderPath: string,
  staged: boolean,
  scopePath: string | null = null,
  initialFile: string | null = null,
  temporary = true
): TabId {
  return addContentTab(
    'diff',
    (id): DiffTab => ({
      kind: 'diff',
      id,
      folderPath,
      source: { kind: 'working', staged, scopePath, revealNonce: generateRevealNonce() },
      initialFile,
      temporary,
      locked: false
    }),
    temporary,
    (t) =>
      t.kind === 'diff' &&
      t.folderPath === folderPath &&
      t.source.kind === 'working' &&
      t.source.staged === staged &&
      t.source.scopePath === scopePath &&
      !t.temporary,
    (t) =>
      t.kind === 'diff' && t.source.kind === 'working'
        ? {
            ...t,
            source: { kind: 'working', staged, scopePath, revealNonce: generateRevealNonce() },
            initialFile
          }
        : t
  )
}

export function addStashTab(
  folderPath: string,
  stashIndex: number,
  message: string,
  initialFile: string | null = null,
  temporary = true
): TabId {
  return addContentTab(
    'diff',
    (id): DiffTab => ({
      kind: 'diff',
      id,
      folderPath,
      source: { kind: 'stash', stashIndex, message },
      initialFile,
      temporary,
      locked: false
    }),
    temporary,
    (t) =>
      t.kind === 'diff' &&
      t.folderPath === folderPath &&
      t.source.kind === 'stash' &&
      t.source.stashIndex === stashIndex &&
      !t.temporary,
    (t) => (t.kind === 'diff' && t.initialFile !== initialFile ? { ...t, initialFile } : t)
  )
}

export function addTextTab(folderPath: string, filePath: string, temporary = true, tabId?: TabId): TabId {
  return addContentTab(
    'text',
    (id): TextTab => ({
      kind: 'text',
      id,
      folderPath,
      filePath,
      fileName: basename(filePath),
      temporary,
      locked: false
    }),
    temporary,
    (t) => t.kind === 'text' && t.folderPath === folderPath && t.filePath === filePath && !t.gitRef && !t.temporary,
    undefined,
    tabId
  )
}

/**
 * Open a new unsaved text tab ("Untitled-N"). Multiple presses stack
 * rather than dedupe; the tab promotes to a real path on first save.
 */
export function addUntitledTextTab(folderPath: string): TabId {
  untitledCounter += 1
  return insertTab({
    kind: 'text',
    id: generateTabId(),
    folderPath,
    filePath: null,
    fileName: `Untitled-${untitledCounter}`,
    temporary: false,
    locked: false
  })
}

/**
 * Promote an unsaved text tab (filePath=null) to a real path after
 * save-as, or rename an existing text tab's target.
 */
export function renameTextTab(tabId: TabId, nextRelative: string, nextFolder?: string): void {
  updateTab(tabId, (t) =>
    t.kind === 'text'
      ? { ...t, filePath: nextRelative, fileName: basename(nextRelative), folderPath: nextFolder ?? t.folderPath }
      : t
  )
}

/** Open a read-only text tab showing a file at a specific git revision. */
export function addReadonlyTextTab(
  folderPath: string,
  filePath: string,
  gitRef: string,
  refLabel: string,
  temporary = true
): TabId {
  return addContentTab(
    'text',
    (id): TextTab => ({
      kind: 'text',
      id,
      folderPath,
      filePath,
      fileName: basename(filePath),
      temporary,
      locked: false,
      gitRef,
      refLabel
    }),
    temporary,
    (t) =>
      t.kind === 'text' && t.folderPath === folderPath && t.filePath === filePath && t.gitRef === gitRef && !t.temporary
  )
}

/** Focus the folder's new tab page, or create one. One new tab page per folder. */
export function openNewTab(folderPath: string): TabId {
  const existing = workbench.tabs.find((t) => t.kind === 'new-tab' && t.folderPath === folderPath)
  if (existing) {
    setActiveTab(existing.id)
    return existing.id
  }
  const tab: NewTab = { kind: 'new-tab', id: generateTabId(), folderPath, locked: false, temporary: true }
  return insertTab(tab)
}

export function addNotificationToolTab(folderPath: string, temporary = false): TabId {
  return addContentTab(
    'tool',
    (id): ToolTab => ({
      kind: 'tool',
      id,
      folderPath,
      tool: 'notification-test',
      label: 'Notification Tester',
      temporary,
      locked: false
    }),
    temporary,
    (t) => t.kind === 'tool' && t.folderPath === folderPath && t.tool === 'notification-test' && !t.temporary
  )
}

export function addIssueTab(folderPath: string, issueId: string, title: string, temporary = true): TabId {
  return addContentTab(
    'issue',
    (id): IssueTab => ({ kind: 'issue', id, folderPath, issueId, title, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'issue' && t.folderPath === folderPath && t.issueId === issueId,
    (existing) => (existing.kind === 'issue' && existing.title !== title ? { ...existing, title } : existing)
  )
}

/** Update the cached title on an existing issue tab. */
export function updateIssueTabTitle(folderPath: string, issueId: string, title: string): void {
  const tab = workbench.tabs.find((t) => t.kind === 'issue' && t.folderPath === folderPath && t.issueId === issueId)
  if (!tab) return
  updateTab(tab.id, (t) => (t.kind === 'issue' && t.title !== title ? { ...t, title } : t))
}

export function addEpicTab(folderPath: string, epicId: string, title: string, temporary = true): TabId {
  return addContentTab(
    'epic',
    (id): EpicTab => ({ kind: 'epic', id, folderPath, epicId, title, temporary, locked: false }),
    temporary,
    (t) => t.kind === 'epic' && t.folderPath === folderPath && t.epicId === epicId,
    (existing) => (existing.kind === 'epic' && existing.title !== title ? { ...existing, title } : existing)
  )
}

/** Update the cached title on an existing epic tab. */
export function updateEpicTabTitle(folderPath: string, epicId: string, title: string): void {
  const tab = workbench.tabs.find((t) => t.kind === 'epic' && t.folderPath === folderPath && t.epicId === epicId)
  if (!tab) return
  updateTab(tab.id, (t) => (t.kind === 'epic' && t.title !== title ? { ...t, title } : t))
}

// PROMOTION //
export function promoteTab(tabId: TabId): void {
  updateTab(tabId, (t) =>
    t.kind === 'session' || t.kind === 'new-tab' || t.kind === 'task' || !t.temporary ? t : { ...t, temporary: false }
  )
}

export function promoteTabWhenReady(pendingTabId: TabId | Promise<TabId> | null | undefined): void {
  if (!pendingTabId) return
  void Promise.resolve(pendingTabId)
    .then((tabId) => promoteTab(tabId))
    .catch(() => {})
}

// CLOSE / REOPEN //
export function closeTab(tabId: TabId): void {
  const index = workbench.tabs.findIndex((t) => t.id === tabId)
  if (index < 0) return
  const tab = workbench.tabs[index]
  if (tab.locked || isTabInert(tab)) return

  // Active local and remote tasks survive view teardown. Explicit close
  // stops them and intentionally omits them from reopen history.
  const snapshot = tab.kind === 'task' ? null : tabToPersisted(tab)
  if (snapshot) pushClosedTab(snapshot)

  if (tab.kind === 'session') {
    sessionRegistry.dispose(tab.id)
  } else if (tab.kind === 'task') {
    taskRegistry.dispose(tab.runId)
  }
  if (platform.capabilities.fileClaims && tab.kind === 'text' && tab.filePath != null && !tab.gitRef) {
    void requireNative().files.releaseFile(resolveProjectFile(tab.folderPath, tab.filePath))
  }

  const tabs = workbench.tabs.filter((t) => t.id !== tabId)
  let activeTabId = workbench.activeTabId
  if (activeTabId === tabId) {
    activeTabId = (tabs[index] ?? tabs[index - 1])?.id ?? null
  }
  if (lastActiveByFolder.get(tab.folderPath) === tabId) lastActiveByFolder.delete(tab.folderPath)
  commit({ tabs, activeTabId })
}

/**
 * Pop the most recently closed tab and re-add it via the normal add*
 * function for that kind. Returns the new tab id, or null when the stack
 * is empty. Mirrors VSCode's Ctrl+Shift+T.
 */
export async function reopenLastClosedTab(): Promise<TabId | null> {
  const head = closedTabs[0]
  if (!head) return null
  closedTabs = closedTabs.slice(1)

  try {
    switch (head.kind) {
      // Closing killed this run. Reopen resumes provider history under a fresh PTY id.
      case 'session':
        return addSessionTab(head.folderPath, {
          providerId: head.providerId,
          title: head.title,
          resumeToken: head.resumeToken
        })
      case 'task':
        // Task snapshots never enter closedTabs; keep the persisted union exhaustive.
        return null
      case 'text':
        return head.gitRef
          ? addReadonlyTextTab(head.folderPath, head.filePath, head.gitRef, head.refLabel ?? head.gitRef, false)
          : await openTextFile(head.folderPath, head.filePath, { temporary: false })
      case 'diff':
        switch (head.source.kind) {
          case 'working':
            return addChangesTab(
              head.folderPath,
              head.source.staged,
              head.source.scopePath ?? null,
              head.initialFile,
              false
            )
          case 'commit':
            return addCommitTab(
              head.folderPath,
              head.source.commitHash,
              head.source.shortHash,
              head.source.message,
              head.initialFile,
              false
            )
          case 'stash':
            return addStashTab(head.folderPath, head.source.stashIndex, head.source.message, head.initialFile, false)
          default: {
            const _exhaustive: never = head.source
            return _exhaustive
          }
        }
      case 'new-tab':
        return openNewTab(head.folderPath)
      case 'issue':
        return addIssueTab(head.folderPath, head.issueId, head.title, false)
      case 'epic':
        return addEpicTab(head.folderPath, head.epicId, head.title, false)
      default: {
        const _exhaustive: never = head
        return _exhaustive
      }
    }
  } catch (e) {
    closedTabs = [head, ...closedTabs]
    throw e
  }
}
