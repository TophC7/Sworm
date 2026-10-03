// Workbench persistence — debounced save/restore of this window's tab list.
//
// Persistence is part of normal operation, not a reload-only hook, so a
// crash or force-quit can never drop more than one debounce window of
// state. Each workbench stores its own blob under `workbench:<id>`.

import { backend } from '$lib/api/backend'
import {
  isProcessLive,
  type PersistedTab,
  type PersistedWorkbenchV4,
  type Tab,
  type Workbench
} from '$lib/features/workbench/model'
import { basename } from '$lib/utils/paths'
import { flushActiveGroups } from './groups.svelte'

const WORKBENCH_DEBOUNCE_MS = 250

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

export function tabToPersisted(tab: Tab): PersistedTab | null {
  switch (tab.kind) {
    case 'session':
      return {
        kind: 'session',
        folderPath: tab.folderPath,
        title: tab.title,
        providerId: tab.providerId,
        runId: tab.runId,
        resumeToken: tab.resumeToken,
        locked: tab.locked
      }
    case 'text':
      // Untitled / new-empty buffers have no filePath yet and can't be
      // round-tripped to disk — drop them from persistence and the
      // closed-tab stack.
      if (tab.filePath == null) return null
      return {
        kind: 'text',
        folderPath: tab.folderPath,
        filePath: tab.filePath,
        gitRef: tab.gitRef,
        refLabel: tab.refLabel,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'diff':
      return {
        kind: 'diff',
        folderPath: tab.folderPath,
        source:
          tab.source.kind === 'working'
            ? {
                kind: 'working',
                staged: tab.source.staged,
                scopePath: tab.source.scopePath
              }
            : tab.source.kind === 'commit'
              ? {
                  kind: 'commit',
                  commitHash: tab.source.commitHash,
                  shortHash: tab.source.shortHash,
                  message: tab.source.message
                }
              : {
                  kind: 'stash',
                  stashIndex: tab.source.stashIndex,
                  message: tab.source.message
                },
        initialFile: tab.initialFile,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'tool':
      // Dev-only tab; don't let it show up on next launch.
      return null
    case 'issue':
      // Title is a cache; on hydrate we re-fetch detail and refresh it.
      return {
        kind: 'issue',
        folderPath: tab.folderPath,
        issueId: tab.issueId,
        title: tab.title,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'epic':
      return {
        kind: 'epic',
        folderPath: tab.folderPath,
        epicId: tab.epicId,
        title: tab.title,
        temporary: tab.temporary,
        locked: tab.locked
      }
    case 'new-tab':
      // Persisted so a folder whose only tab is the new tab page survives a
      // restart instead of silently vanishing from the tab strip.
      return { kind: 'new-tab', folderPath: tab.folderPath, locked: tab.locked }
    case 'task':
      // Only active runs belong in the snapshot, regardless of host type.
      if (!isProcessLive(tab.status)) return null
      return {
        kind: 'task',
        folderPath: tab.folderPath,
        runId: tab.runId,
        taskId: tab.taskId,
        activeFilePath: tab.activeFilePath,
        label: tab.label,
        icon: tab.icon,
        group: tab.group,
        locked: tab.locked
      }
    default: {
      const _exhaustive: never = tab
      return _exhaustive
    }
  }
}

export function serializeWorkbench(wb: Workbench): PersistedWorkbenchV4 {
  const tabs: PersistedTab[] = []
  let activeTabIndex = -1
  for (const tab of wb.tabs) {
    const persisted = tabToPersisted(tab)
    if (!persisted) continue
    if (tab.id === wb.activeTabId) activeTabIndex = tabs.length
    tabs.push(persisted)
  }
  // If the active tab was dropped from persistence (for example, an
  // untitled buffer or completed task), restore the last persisted tab.
  if (activeTabIndex < 0 && tabs.length > 0) activeTabIndex = tabs.length - 1
  return { version: 4, activeTabIndex, tabs }
}

export function persistedToTab(persisted: PersistedTab, id: string): Tab {
  switch (persisted.kind) {
    case 'session':
      return {
        kind: 'session',
        id,
        folderPath: persisted.folderPath,
        title: persisted.title,
        providerId: persisted.providerId,
        runId: persisted.runId ?? null,
        resumeToken: persisted.resumeToken,
        // Restored processes start lazily on first activation.
        status: 'dormant',
        locked: persisted.locked
      }
    case 'task':
      return {
        kind: 'task',
        id,
        folderPath: persisted.folderPath,
        runId: persisted.runId,
        attachOnly: true,
        taskId: persisted.taskId,
        activeFilePath: persisted.activeFilePath,
        label: persisted.label,
        icon: persisted.icon,
        group: persisted.group,
        // Restored runs reattach or fail; a missing run never executes again.
        status: 'starting',
        exitCode: null,
        locked: persisted.locked
      }
    case 'text':
      return {
        kind: 'text',
        id,
        folderPath: persisted.folderPath,
        filePath: persisted.filePath,
        fileName: basename(persisted.filePath),
        temporary: persisted.temporary,
        locked: persisted.locked,
        gitRef: persisted.gitRef,
        refLabel: persisted.refLabel
      }
    case 'diff':
      return {
        kind: 'diff',
        id,
        folderPath: persisted.folderPath,
        source:
          persisted.source.kind === 'working'
            ? {
                kind: 'working',
                staged: persisted.source.staged,
                scopePath: persisted.source.scopePath ?? null,
                revealNonce: 0
              }
            : persisted.source.kind === 'commit'
              ? {
                  kind: 'commit',
                  commitHash: persisted.source.commitHash,
                  shortHash: persisted.source.shortHash,
                  message: persisted.source.message
                }
              : {
                  kind: 'stash',
                  stashIndex: persisted.source.stashIndex,
                  message: persisted.source.message
                },
        initialFile: persisted.initialFile,
        temporary: persisted.temporary,
        locked: persisted.locked
      }
    case 'new-tab':
      return { kind: 'new-tab', id, folderPath: persisted.folderPath, locked: persisted.locked, temporary: true }
    case 'issue':
      return {
        kind: 'issue',
        id,
        folderPath: persisted.folderPath,
        issueId: persisted.issueId,
        title: persisted.title,
        temporary: persisted.temporary,
        locked: persisted.locked
      }
    case 'epic':
      return {
        kind: 'epic',
        id,
        folderPath: persisted.folderPath,
        epicId: persisted.epicId,
        title: persisted.title,
        temporary: persisted.temporary,
        locked: persisted.locked
      }
    default: {
      const _exhaustive: never = persisted
      return _exhaustive
    }
  }
}

// ---------------------------------------------------------------------------
// Debounced persistence
// ---------------------------------------------------------------------------

export interface WorkbenchWriter {
  schedule(produce: () => PersistedWorkbenchV4): void
  flush(): Promise<void>
  stop(): void
}

/** One ordered, deduplicated writer per persistence key. */
export function createWorkbenchWriter(save: (json: string) => Promise<void>): WorkbenchWriter {
  let timer: ReturnType<typeof setTimeout> | undefined
  let pending: (() => PersistedWorkbenchV4) | null = null
  let lastWrittenJson: string | null = null
  let stopped = false
  let inFlight: Promise<void> | null = null

  async function flush(): Promise<void> {
    clearTimeout(timer)
    timer = undefined
    for (;;) {
      if (stopped) throw new Error('Workbench is no longer controlled by this window')
      if (inFlight) {
        await inFlight
        continue
      }
      const produce = pending
      if (!produce) return
      pending = null
      const json = JSON.stringify(produce())
      if (json === lastWrittenJson) continue
      const write = save(json).then(
        () => {
          lastWrittenJson = json
        },
        (error) => {
          if (pending === null && !stopped) pending = produce
          throw error
        }
      )
      inFlight = write
      try {
        await write
      } finally {
        if (inFlight === write) inFlight = null
      }
    }
  }

  return {
    schedule(produce: () => PersistedWorkbenchV4) {
      if (stopped) return
      pending = produce
      clearTimeout(timer)
      timer = setTimeout(
        () => void flush().catch((error) => console.warn('Workbench persist failed:', error)),
        WORKBENCH_DEBOUNCE_MS
      )
    },
    flush,
    stop() {
      stopped = true
      pending = null
      clearTimeout(timer)
      timer = undefined
    }
  }
}

const localWriter = createWorkbenchWriter((json) => backend.app.statePut(`workbench:${localWorkbenchId}`, json))
let localWorkbenchId = 'main'

export function schedulePersistWorkbench(workbenchId: string, produce: () => PersistedWorkbenchV4): void {
  localWorkbenchId = workbenchId
  localWriter.schedule(produce)
}

export async function flushWorkbench(workbenchId: string): Promise<void> {
  localWorkbenchId = workbenchId
  await localWriter.flush()
  await flushActiveGroups()
}

/** Permanently suspend this document's persistence after web takeover/Close. */
export function stopWorkbenchPersistence(): void {
  localWriter.stop()
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

// Pre-release: any other version is treated as absent, no migration.
export function parsePersistedWorkbench(value: unknown): PersistedWorkbenchV4 | null {
  if (!value || typeof value !== 'object') return null
  const obj = value as Record<string, unknown>
  if (obj.version !== 4 || !Array.isArray(obj.tabs) || typeof obj.activeTabIndex !== 'number') return null
  const snapshot = obj as unknown as PersistedWorkbenchV4
  // Drop unknown tab kinds at the shared local/server restore boundary.
  // Preserve the active tab, or choose its next surviving neighbour.
  let activeTabIndex = snapshot.activeTabIndex < 0 ? -1 : 0
  const tabs = snapshot.tabs.filter((tab, index) => {
    if (!tab || !['session', 'task', 'text', 'diff', 'new-tab', 'issue', 'epic'].includes(tab.kind)) return false
    if (index < snapshot.activeTabIndex) activeTabIndex += 1
    return true
  })
  return { ...snapshot, tabs, activeTabIndex: Math.min(activeTabIndex, tabs.length - 1) }
}

export async function loadPersistedWorkbench(workbenchId: string): Promise<PersistedWorkbenchV4 | null> {
  try {
    const raw = await backend.app.stateGet(`workbench:${workbenchId}`)
    if (!raw) return null
    const parsed = parsePersistedWorkbench(JSON.parse(raw))
    if (!parsed) {
      console.warn('Discarding malformed workbench blob')
      return null
    }
    return parsed
  } catch (error) {
    console.warn('Failed to load workbench blob:', error)
    return null
  }
}
