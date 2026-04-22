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

export type Tab = SessionTab | DiffTab | TextTab | ToolTab | LauncherTab

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
  return tab.kind === 'session' || tab.kind === 'text'
}
