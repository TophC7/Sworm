// Workbench model — shared types and pure helpers.
//
// Keep this module free of Svelte runes and side effects so persistence,
// presentation, and DnD code can depend on workbench shapes without
// importing the state store itself.

export type TabId = string

export type PaneSlot = 'sole' | 'left' | 'right' | 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right'
export type SplitDirection = 'left' | 'right' | 'up' | 'down'

export interface SessionTab {
  kind: 'session'
  id: TabId
  sessionId: string
  title: string
  providerId: string
  locked: boolean
}

export interface WorkingDiffSource {
  kind: 'working'
  staged: boolean
  scopePath: string | null
  revealNonce: number
}

export interface CommitDiffSource {
  kind: 'commit'
  commitHash: string
  shortHash: string
  message: string
}

export interface StashDiffSource {
  kind: 'stash'
  stashIndex: number
  message: string
}

export type DiffSource = WorkingDiffSource | CommitDiffSource | StashDiffSource

export interface DiffTab {
  kind: 'diff'
  id: TabId
  source: DiffSource
  initialFile: string | null
  temporary: boolean
  locked: boolean
}

export interface TextTab {
  kind: 'text'
  id: TabId
  filePath: string | null
  fileName: string
  temporary: boolean
  locked: boolean
  gitRef?: string
  refLabel?: string
}

export interface ToolTab {
  kind: 'tool'
  id: TabId
  tool: 'notification-test'
  label: string
  temporary: boolean
  locked: boolean
}

export interface LauncherTab {
  kind: 'launcher'
  id: TabId
  locked: boolean
  temporary: false
}

export type TaskRunStatus = 'starting' | 'running' | 'exited' | 'failed'

export interface TaskTab {
  kind: 'task'
  id: TabId
  /** Frontend-generated UUID used as the PTY key for the live run. */
  runId: string
  /** Stable task id from .sworm/tasks.json; used to re-resolve on restart. */
  taskId: string
  /** Active editor path captured when the run was launched. */
  activeFilePath: string | null
  /** Cached label from the task definition (display-only; refreshes on reload). */
  label: string
  /** Cached Lucide icon name. */
  icon: string | null
  /** Optional group label used by the launcher and menus. */
  group: string | null
  status: TaskRunStatus
  exitCode: number | null
  locked: boolean
}

export type Tab = SessionTab | DiffTab | TextTab | ToolTab | LauncherTab | TaskTab

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
  focusedPaneSlot: PaneSlot
}

export type PersistedTab =
  | { kind: 'session'; sessionId: string; title: string; providerId: string; locked: boolean }
  | {
      kind: 'text'
      filePath: string
      gitRef?: string
      refLabel?: string
      temporary: boolean
      locked: boolean
    }
  | {
      kind: 'diff'
      source:
        | {
            kind: 'working'
            staged: boolean
            scopePath?: string | null
          }
        | {
            kind: 'commit'
            commitHash: string
            shortHash: string
            message: string
          }
        | {
            kind: 'stash'
            stashIndex: number
            message: string
          }
      initialFile: string | null
      temporary: boolean
      locked: boolean
    }
  | { kind: 'tool'; tool: 'notification-test'; label: string; temporary: boolean; locked: boolean }

export interface PersistedWorkspaceV2 {
  version: 2
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

export function createPane(slot: PaneSlot): PaneState {
  return { slot, tabs: [], activeTabId: null }
}

export function canLockTab(tab: Tab): boolean {
  return tab.kind === 'session' || tab.kind === 'text' || tab.kind === 'task'
}
