// Workspace persistence — debounced save/restore for tab+pane layout
// and app-shell state (which projects are open, which is active).
//
// The plan wants persistence to be part of normal operation, not a
// reload-only hook, so a crash or force-quit can never drop more
// than one debounce window of state.

import { backend } from '$lib/api/backend'
import type {
  PaneSlot,
  PersistedTab,
  PersistedWorkspaceV1,
  ProjectWorkspace,
  QuadLayout,
  SplitMode,
  Tab
} from '$lib/stores/workspace.svelte'

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const APP_STATE_KEY_OPEN_PROJECTS = 'open_project_ids'
const APP_STATE_KEY_ACTIVE_PROJECT = 'active_project_id'

const WORKSPACE_DEBOUNCE_MS = 250
const APP_SHELL_DEBOUNCE_MS = 250

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

export function tabToPersisted(tab: Tab): PersistedTab | null {
  switch (tab.kind) {
    case 'session':
      return {
        kind: 'session',
        sessionId: tab.sessionId,
        title: tab.title,
        providerId: tab.providerId,
        locked: tab.locked
      }
    case 'editor':
      // Untitled / new-empty buffers have no filePath yet and can't be
      // round-tripped to disk — drop them from persistence and the
      // closed-tab stack.
      if (tab.filePath == null) return null
      return {
        kind: 'editor',
        filePath: tab.filePath,
        gitRef: tab.gitRef,
        refLabel: tab.refLabel,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'commit':
      return {
        kind: 'commit',
        commitHash: tab.commitHash,
        shortHash: tab.shortHash,
        message: tab.message,
        initialFile: tab.initialFile,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'changes':
      return {
        kind: 'changes',
        staged: tab.staged,
        initialFile: tab.initialFile,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'stash':
      return {
        kind: 'stash',
        stashIndex: tab.stashIndex,
        message: tab.message,
        initialFile: tab.initialFile,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'notification-test':
      // Dev-only tab; don't let it show up on next launch.
      return null
    default: {
      // Exhaustiveness: any new Tab kind forces the compiler to add a
      // case above. Without this the default path would silently drop
      // new kinds and callers would never know their tab vanished.
      const _exhaustive: never = tab
      return _exhaustive
    }
  }
}

export function serializeWorkspace(ws: ProjectWorkspace, focusedPaneSlot: PaneSlot): PersistedWorkspaceV1 {
  // Tabs are persisted by index — id-based references would be unstable
  // across restore (we mint fresh tab ids on hydrate), so panes record
  // the position of each tab inside the `tabs` array.
  const tabIndexById = new Map<string, number>()
  const persistedTabs: PersistedTab[] = []
  for (const tab of ws.tabs) {
    const persisted = tabToPersisted(tab)
    if (!persisted) continue
    tabIndexById.set(tab.id, persistedTabs.length)
    persistedTabs.push(persisted)
  }

  const panes = ws.panes.map((pane) => {
    const tabIndices: number[] = []
    for (const tabId of pane.tabs) {
      const idx = tabIndexById.get(tabId)
      if (idx !== undefined) tabIndices.push(idx)
    }
    let activeTabIndex = -1
    if (pane.activeTabId) {
      const idx = tabIndexById.get(pane.activeTabId)
      if (idx !== undefined) activeTabIndex = tabIndices.indexOf(idx)
    }
    return {
      slot: pane.slot,
      activeTabIndex,
      tabIndices
    }
  })

  return {
    version: 1,
    focusedPaneSlot,
    splitMode: ws.splitMode,
    quadLayout: ws.quadLayout,
    panes,
    tabs: persistedTabs
  }
}

export interface DeserializedWorkspace {
  tabs: Tab[]
  panes: ProjectWorkspace['panes']
  splitMode: SplitMode
  quadLayout: QuadLayout
  focusedPaneSlot: PaneSlot
}

export function deserializeWorkspace(data: PersistedWorkspaceV1, generateTabId: () => string): DeserializedWorkspace {
  const newIdByOldIndex = new Map<number, string>()
  const tabs: Tab[] = []

  data.tabs.forEach((persisted, oldIndex) => {
    const id = generateTabId()
    let tab: Tab | null = null

    switch (persisted.kind) {
      case 'session':
        tab = {
          kind: 'session',
          id,
          sessionId: persisted.sessionId,
          title: persisted.title,
          providerId: persisted.providerId,
          locked: persisted.locked
        }
        break
      case 'editor': {
        const fileName = persisted.filePath.split('/').pop() ?? persisted.filePath
        tab = {
          kind: 'editor',
          id,
          filePath: persisted.filePath,
          fileName,
          temporary: persisted.temporary,
          locked: persisted.locked,
          gitRef: persisted.gitRef,
          refLabel: persisted.refLabel
        }
        break
      }
      case 'commit':
        tab = {
          kind: 'commit',
          id,
          commitHash: persisted.commitHash,
          shortHash: persisted.shortHash,
          message: persisted.message,
          initialFile: persisted.initialFile,
          temporary: persisted.temporary,
          locked: persisted.locked
        }
        break
      case 'changes':
        tab = {
          kind: 'changes',
          id,
          label: persisted.staged ? 'Staged Changes' : 'Changes',
          staged: persisted.staged,
          initialFile: persisted.initialFile,
          temporary: persisted.temporary,
          locked: persisted.locked
        }
        break
      case 'stash':
        tab = {
          kind: 'stash',
          id,
          stashIndex: persisted.stashIndex,
          message: persisted.message,
          initialFile: persisted.initialFile,
          temporary: persisted.temporary,
          locked: persisted.locked
        }
        break
      case 'notification-test':
        // Legacy blobs may still contain this; we stopped persisting
        // the dev-only tab, so drop it on restore too.
        tab = null
        break
      default: {
        const _exhaustive: never = persisted
        tab = _exhaustive
      }
    }

    if (tab) {
      tabs.push(tab)
      newIdByOldIndex.set(oldIndex, id)
    }
  })

  const panes = data.panes.map((pane) => {
    const tabIds: string[] = []
    for (const oldIdx of pane.tabIndices) {
      const id = newIdByOldIndex.get(oldIdx)
      if (id) tabIds.push(id)
    }
    const activeTabId = pane.activeTabIndex >= 0 ? (tabIds[pane.activeTabIndex] ?? null) : null
    return {
      slot: pane.slot,
      tabs: tabIds,
      activeTabId
    }
  })

  return {
    tabs,
    panes: panes.length > 0 ? panes : [{ slot: 'sole' as PaneSlot, tabs: [], activeTabId: null }],
    splitMode: data.splitMode,
    quadLayout: data.quadLayout,
    focusedPaneSlot: data.focusedPaneSlot
  }
}

// ---------------------------------------------------------------------------
// Debounced persistence
// ---------------------------------------------------------------------------

/**
 * Per-key debounced writer. Producers return the payload to persist,
 * or `null` to skip the write (e.g. the project was closed mid-debounce
 * and there's nothing coherent to persist for that id anymore).
 */
class DebouncedWriter<K, T> {
  private readonly timers = new Map<K, ReturnType<typeof setTimeout>>()
  private readonly pending = new Map<K, () => T | null>()

  constructor(
    private readonly delayMs: number,
    private readonly write: (key: K, value: T) => Promise<void>,
    private readonly onError: (key: K, error: unknown) => void
  ) {}

  schedule(key: K, produce: () => T | null): void {
    this.pending.set(key, produce)
    const existing = this.timers.get(key)
    if (existing) clearTimeout(existing)
    this.timers.set(
      key,
      setTimeout(() => {
        void this.flush(key)
      }, this.delayMs)
    )
  }

  async flush(key: K): Promise<void> {
    const produce = this.pending.get(key)
    this.pending.delete(key)
    const timer = this.timers.get(key)
    if (timer) {
      clearTimeout(timer)
      this.timers.delete(key)
    }
    if (!produce) return
    try {
      const value = produce()
      if (value === null) return
      await this.write(key, value)
    } catch (error) {
      this.onError(key, error)
    }
  }

  async flushAll(): Promise<void> {
    await Promise.all(Array.from(this.pending.keys()).map((key) => this.flush(key)))
  }
}

const workspaceWriter = new DebouncedWriter<string, PersistedWorkspaceV1>(
  WORKSPACE_DEBOUNCE_MS,
  async (projectId, payload) => {
    await backend.workspace.putState(projectId, JSON.stringify(payload))
  },
  (id, err) => console.warn(`Workspace persist failed for ${id}:`, err)
)

interface AppShellState {
  openProjectIds: string[]
  activeProjectId: string | null
}

// App-shell is a single global key, so the keyed DebouncedWriter would
// need a sentinel. Plain closure state is simpler and lighter.
let appShellTimer: ReturnType<typeof setTimeout> | null = null
let appShellPending: AppShellState | null = null

async function writeAppShell(state: AppShellState): Promise<void> {
  await Promise.all([
    backend.workspace.appStatePut(APP_STATE_KEY_OPEN_PROJECTS, JSON.stringify(state.openProjectIds)),
    backend.workspace.appStatePut(APP_STATE_KEY_ACTIVE_PROJECT, JSON.stringify(state.activeProjectId))
  ])
}

export function schedulePersistWorkspace(projectId: string, produce: () => PersistedWorkspaceV1 | null): void {
  workspaceWriter.schedule(projectId, produce)
}

export function flushWorkspace(projectId: string): Promise<void> {
  return workspaceWriter.flush(projectId)
}

export function schedulePersistAppShell(state: AppShellState): void {
  appShellPending = { openProjectIds: [...state.openProjectIds], activeProjectId: state.activeProjectId }
  if (appShellTimer) clearTimeout(appShellTimer)
  appShellTimer = setTimeout(() => {
    void flushAppShell()
  }, APP_SHELL_DEBOUNCE_MS)
}

export async function flushAppShell(): Promise<void> {
  const state = appShellPending
  appShellPending = null
  if (appShellTimer) {
    clearTimeout(appShellTimer)
    appShellTimer = null
  }
  if (!state) return
  try {
    await writeAppShell(state)
  } catch (error) {
    console.warn('App-shell persist failed:', error)
  }
}

export async function flushAllWorkspaces(): Promise<void> {
  await workspaceWriter.flushAll()
  await flushAppShell()
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

function isPersistedWorkspaceShape(value: unknown): value is PersistedWorkspaceV1 {
  if (!value || typeof value !== 'object') return false
  const obj = value as Record<string, unknown>
  if (obj.version !== 1) return false
  if (!Array.isArray(obj.tabs)) return false
  if (!Array.isArray(obj.panes)) return false
  return true
}

export async function loadPersistedWorkspace(projectId: string): Promise<PersistedWorkspaceV1 | null> {
  try {
    const raw = await backend.workspace.getState(projectId)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!isPersistedWorkspaceShape(parsed)) {
      console.warn(`Discarding malformed workspace blob for ${projectId}`)
      return null
    }
    return parsed
  } catch (error) {
    console.warn(`Failed to load workspace blob for ${projectId}:`, error)
    return null
  }
}

export async function loadPersistedAppShell(): Promise<AppShellState> {
  const fallback: AppShellState = { openProjectIds: [], activeProjectId: null }
  try {
    const [rawOpen, rawActive] = await Promise.all([
      backend.workspace.appStateGet(APP_STATE_KEY_OPEN_PROJECTS),
      backend.workspace.appStateGet(APP_STATE_KEY_ACTIVE_PROJECT)
    ])
    const parsedOpen: unknown = rawOpen ? JSON.parse(rawOpen) : []
    const parsedActive: unknown = rawActive ? JSON.parse(rawActive) : null
    return {
      openProjectIds: Array.isArray(parsedOpen) ? parsedOpen.filter((id): id is string => typeof id === 'string') : [],
      activeProjectId:
        typeof parsedActive === 'string' || parsedActive === null ? (parsedActive as string | null) : null
    }
  } catch (error) {
    console.warn('App-shell restore failed:', error)
    return fallback
  }
}
