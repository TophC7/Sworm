// Workbench persistence — debounced save/restore of the global tab list.
//
// Persistence is part of normal operation, not a reload-only hook, so a
// crash or force-quit can never drop more than one debounce window of
// state. One blob under the `workbench` app-state key holds every tab.

import { backend } from '$lib/api/backend'
import type { PersistedTab, PersistedWorkbenchV3, Tab, Workbench } from '$lib/features/workbench/model'
import { basename } from '$lib/utils/paths'

const APP_STATE_KEY_WORKBENCH = 'workbench'
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
        sessionId: tab.sessionId,
        title: tab.title,
        providerId: tab.providerId,
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
    case 'launcher':
      // Persisted so a folder whose only tab is the launcher survives a
      // restart instead of silently vanishing from the tab strip.
      return { kind: 'launcher', folderPath: tab.folderPath, locked: tab.locked }
    case 'task':
      // Tasks are ephemeral runs. Their PTY is tied to a live runId
      // that dies with the process, so persisting the tab across
      // restarts would resurrect a dead handle.
      return null
    default: {
      const _exhaustive: never = tab
      return _exhaustive
    }
  }
}

export function serializeWorkbench(wb: Workbench): PersistedWorkbenchV3 {
  const tabs: PersistedTab[] = []
  let activeTabIndex = -1
  for (const tab of wb.tabs) {
    const persisted = tabToPersisted(tab)
    if (!persisted) continue
    if (tab.id === wb.activeTabId) activeTabIndex = tabs.length
    tabs.push(persisted)
  }
  // If the active tab was dropped from persistence (untitled buffer, task
  // run), fall back to the last persisted tab so restore lands on
  // something instead of an empty surface.
  if (activeTabIndex < 0 && tabs.length > 0) activeTabIndex = tabs.length - 1
  return { version: 3, activeTabIndex, tabs }
}

export function persistedToTab(persisted: PersistedTab, id: string): Tab {
  switch (persisted.kind) {
    case 'session':
      return {
        kind: 'session',
        id,
        folderPath: persisted.folderPath,
        sessionId: persisted.sessionId,
        title: persisted.title,
        providerId: persisted.providerId,
        resumeToken: persisted.resumeToken,
        // Restored processes start lazily on first activation.
        status: 'dormant',
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
    case 'launcher':
      return { kind: 'launcher', id, folderPath: persisted.folderPath, locked: persisted.locked, temporary: false }
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

let timer: ReturnType<typeof setTimeout> | undefined
let pending: (() => PersistedWorkbenchV3) | null = null
// Session/task status ticks commit the workbench without changing the
// persisted shape; skip the SQLite write when the blob is byte-identical.
let lastWrittenJson: string | null = null

export function schedulePersistWorkbench(produce: () => PersistedWorkbenchV3): void {
  pending = produce
  clearTimeout(timer)
  timer = setTimeout(() => {
    void flushWorkbench()
  }, WORKBENCH_DEBOUNCE_MS)
}

/** Force an immediate write of any pending mutation (managed reload, app exit). */
export async function flushWorkbench(): Promise<void> {
  const produce = pending
  pending = null
  clearTimeout(timer)
  timer = undefined
  if (!produce) return
  const json = JSON.stringify(produce())
  if (json === lastWrittenJson) return
  try {
    await backend.app.statePut(APP_STATE_KEY_WORKBENCH, json)
    lastWrittenJson = json
  } catch (error) {
    console.warn('Workbench persist failed:', error)
  }
}

// ---------------------------------------------------------------------------
// Restore
// ---------------------------------------------------------------------------

function isPersistedWorkbenchShape(value: unknown): value is PersistedWorkbenchV3 {
  if (!value || typeof value !== 'object') return false
  const obj = value as Record<string, unknown>
  return obj.version === 3 && Array.isArray(obj.tabs) && typeof obj.activeTabIndex === 'number'
}

export async function loadPersistedWorkbench(): Promise<PersistedWorkbenchV3 | null> {
  try {
    const raw = await backend.app.stateGet(APP_STATE_KEY_WORKBENCH)
    if (!raw) return null
    const parsed: unknown = JSON.parse(raw)
    if (!isPersistedWorkbenchShape(parsed)) {
      console.warn('Discarding malformed workbench blob')
      return null
    }
    return parsed
  } catch (error) {
    console.warn('Failed to load workbench blob:', error)
    return null
  }
}
