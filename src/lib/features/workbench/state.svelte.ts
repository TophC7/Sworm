// Workbench state — the single global tab list.
//
// Every tab carries its own `folderPath`; the active tab decides which
// folder the sidebar, status bar, and folder-scoped commands operate on.
// There is no project lifecycle: opening a folder either focuses one of
// its tabs or seeds a launcher tab for it.

import { backend } from '$lib/api/backend'
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
  LauncherTab,
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
import { canLockTab } from '$lib/features/workbench/model'
import {
  loadPersistedWorkbench,
  persistedToTab,
  schedulePersistWorkbench,
  serializeWorkbench,
  tabToPersisted
} from '$lib/features/workbench/persistence'
import { basename } from '$lib/utils/paths'

export type {
  DiffSource,
  DiffTab,
  EpicTab,
  IssueTab,
  LauncherTab,
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

// Monotonic counter for "Untitled-N" labels on new untitled text tabs.
let untitledCounter = 0

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

function findTab(tabId: TabId): Tab | undefined {
  return workbench.tabs.find((t) => t.id === tabId)
}

/**
 * Single choke point for layout mutation: reassigns the workbench so
 * `$derived` consumers see a new reference, then schedules a debounced
 * persist. Callers mutate `tabs`/`activeTabId` via the `next` object and
 * identify folders whose tabs they removed. A folder that loses its last
 * tab releases every folder-scoped cache and process (git, nix, LSP,
 * providers, issue bridge). Reopening it later repopulates them lazily.
 */
function commit(
  next: { tabs?: Tab[]; activeTabId?: TabId | null } = {},
  possiblyReleasedFolderPaths: readonly string[] = []
): void {
  const tabs = next.tabs ?? workbench.tabs
  const activeTabId = next.activeTabId === undefined ? workbench.activeTabId : next.activeTabId
  workbench = { tabs: [...tabs], activeTabId }
  const active = activeTabId ? tabs.find((t) => t.id === activeTabId) : undefined
  if (active) lastActiveByFolder.set(active.folderPath, active.id)
  for (const folderPath of possiblyReleasedFolderPaths) {
    if (!tabs.some((tab) => tab.folderPath === folderPath)) releaseFolder(folderPath)
  }
  if (restored) schedulePersistWorkbench(() => serializeWorkbench(workbench))
}

/**
 * Append and activate a tab. Launcher tabs are transient: the first real
 * tab opened in a folder replaces that folder's launcher.
 */
function appendTab(tab: Tab): TabId {
  const tabs =
    tab.kind === 'launcher'
      ? workbench.tabs
      : workbench.tabs.filter((t) => !(t.kind === 'launcher' && t.folderPath === tab.folderPath))
  commit({ tabs: [...tabs, tab], activeTabId: tab.id })
  return tab.id
}

function updateTab(tabId: TabId, update: (tab: Tab) => Tab): void {
  let dirty = false
  const tabs = workbench.tabs.map((t) => {
    if (t.id !== tabId) return t
    const updated = update(t)
    if (updated !== t) dirty = true
    return updated
  })
  if (dirty) commit({ tabs })
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
  return getRecentFolders().filter((path) => !open.has(path))
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

export function reorderTab(fromIndex: number, toIndex: number): void {
  const tabs = workbench.tabs
  if (fromIndex < 0 || toIndex < 0 || fromIndex >= tabs.length || toIndex >= tabs.length) return
  if (fromIndex === toIndex) return
  const next = [...tabs]
  const [moved] = next.splice(fromIndex, 1)
  next.splice(toIndex, 0, moved)
  commit({ tabs: next })
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
 * active tab or seed a launcher tab when the folder has none.
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
  openLauncherTab(folderPath)
}

// RESTORE //
/**
 * Hydrate the persisted tab list. Tabs whose folder no longer resolves are
 * dropped; session tabs come back dormant and start on first activation.
 */
export async function restoreWorkbench(): Promise<void> {
  try {
    const persisted = await loadPersistedWorkbench()
    if (persisted) {
      const alive = new Set(await filterExistingFolders([...new Set(persisted.tabs.map((t) => t.folderPath))]))

      // Keep each survivor's original index so the saved active slot can
      // fall back to its nearest neighbour (right, then left) when dropped.
      const survivors: Array<{ tab: Tab; index: number }> = []
      persisted.tabs.forEach((entry, index) => {
        if (alive.has(entry.folderPath)) survivors.push({ tab: persistedToTab(entry, generateTabId()), index })
      })
      const tabs = survivors.map((s) => s.tab)
      const active =
        survivors.find((s) => s.index >= persisted.activeTabIndex) ??
        survivors.findLast((s) => s.index < persisted.activeTabIndex)
      const activeTabId = active?.tab.id ?? null

      commit({ tabs, activeTabId })
    }
  } catch (error) {
    console.warn('Workbench restore failed, starting empty:', error)
  } finally {
    restored = true
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
  return appendTab({
    kind: 'session',
    id: generateTabId(),
    folderPath,
    title: init.title,
    providerId: init.providerId,
    resumeToken: init.resumeToken,
    status: 'dormant',
    locked: false
  })
}

export function setSessionTabStatus(tabId: TabId, status: SessionStatus): void {
  updateTab(tabId, (t) => (t.kind === 'session' && t.status !== status ? { ...t, status } : t))
}

export function setSessionTabResumeToken(tabId: TabId, resumeToken: string | null): void {
  updateTab(tabId, (t) => (t.kind === 'session' && t.resumeToken !== resumeToken ? { ...t, resumeToken } : t))
}

/** Drop the tab's token only if it still equals `expectedToken`; a token bound meanwhile wins. */
export function clearSessionTabResumeToken(tabId: TabId, expectedToken: string): void {
  updateTab(tabId, (t) =>
    t.kind === 'session' && t.resumeToken === expectedToken ? { ...t, resumeToken: null } : t
  )
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
  return appendTab({
    kind: 'task',
    id: generateTabId(),
    folderPath,
    runId: init.runId,
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
  updateTab(tabId, (t) => (t.kind === 'task' ? { ...t, status, exitCode } : t))
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
  onReuse?: (existing: Tab) => Tab
): TabId {
  if (temporary) {
    const existingTemp = workbench.tabs.find(
      (t) => t.kind === kind && t.kind !== 'session' && t.kind !== 'task' && t.temporary
    )
    if (existingTemp) {
      const newTab = makeTab(existingTemp.id)
      // Skip mutation when tab data hasn't changed (second click of a
      // double-click on the same file).
      if (!tabDataChanged(existingTemp, newTab)) {
        setActiveTab(existingTemp.id)
        return existingTemp.id
      }
      commit(
        {
          tabs: workbench.tabs.map((t) => (t.id === existingTemp.id ? newTab : t)),
          activeTabId: existingTemp.id
        },
        existingTemp.folderPath === newTab.folderPath ? [] : [existingTemp.folderPath]
      )
      return existingTemp.id
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

  return appendTab(makeTab(generateTabId()))
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

export function addTextTab(folderPath: string, filePath: string, temporary = true): TabId {
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
    (t) => t.kind === 'text' && t.folderPath === folderPath && t.filePath === filePath && !t.gitRef && !t.temporary
  )
}

/**
 * Open a new unsaved text tab ("Untitled-N"). Multiple presses stack
 * rather than dedupe; the tab promotes to a real path on first save.
 */
export function addUntitledTextTab(folderPath: string): TabId {
  untitledCounter += 1
  return appendTab({
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
export function renameTextTab(tabId: TabId, newFilePath: string): void {
  updateTab(tabId, (t) => (t.kind === 'text' ? { ...t, filePath: newFilePath, fileName: basename(newFilePath) } : t))
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

/** Focus the folder's launcher tab, or create one. One launcher per folder. */
export function openLauncherTab(folderPath: string): TabId {
  const existing = workbench.tabs.find((t) => t.kind === 'launcher' && t.folderPath === folderPath)
  if (existing) {
    setActiveTab(existing.id)
    return existing.id
  }
  const tab: LauncherTab = { kind: 'launcher', id: generateTabId(), folderPath, locked: false, temporary: false }
  return appendTab(tab)
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
    t.kind === 'session' || t.kind === 'launcher' || t.kind === 'task' || !t.temporary ? t : { ...t, temporary: false }
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
  if (tab.locked) return

  // Snapshot before teardown; `tabToPersisted` returns null for tabs that
  // can't be meaningfully reopened (untitled buffers, tool tabs, task runs).
  const snapshot = tabToPersisted(tab)
  if (snapshot) pushClosedTab(snapshot)

  if (tab.kind === 'session') {
    sessionRegistry.dispose(tab.id)
  } else if (tab.kind === 'task') {
    taskRegistry.dispose(tab.runId)
  }

  const tabs = workbench.tabs.filter((t) => t.id !== tabId)
  let activeTabId = workbench.activeTabId
  if (activeTabId === tabId) {
    activeTabId = (tabs[index] ?? tabs[index - 1])?.id ?? null
  }
  if (lastActiveByFolder.get(tab.folderPath) === tabId) lastActiveByFolder.delete(tab.folderPath)
  commit({ tabs, activeTabId }, [tab.folderPath])
}

/**
 * Pop the most recently closed tab and re-add it via the normal add*
 * function for that kind. Returns the new tab id, or null when the stack
 * is empty. Mirrors VSCode's Ctrl+Shift+T.
 */
export function reopenLastClosedTab(): TabId | null {
  const [head, ...rest] = closedTabs
  if (!head) return null
  closedTabs = rest

  switch (head.kind) {
    case 'session':
      return addSessionTab(head.folderPath, {
        providerId: head.providerId,
        title: head.title,
        resumeToken: head.resumeToken
      })
    case 'text':
      return head.gitRef
        ? addReadonlyTextTab(head.folderPath, head.filePath, head.gitRef, head.refLabel ?? head.gitRef, false)
        : addTextTab(head.folderPath, head.filePath, false)
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
    case 'launcher':
      return openLauncherTab(head.folderPath)
    case 'issue':
      return addIssueTab(head.folderPath, head.issueId, head.title, false)
    case 'epic':
      return addEpicTab(head.folderPath, head.epicId, head.title, false)
    default: {
      const _exhaustive: never = head
      return _exhaustive
    }
  }
}
