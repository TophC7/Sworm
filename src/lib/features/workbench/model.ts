// Workbench model — shared types and pure helpers.
//
// Keep this module free of Svelte runes and side effects so persistence,
// presentation, and DnD code can depend on workbench shapes without
// importing the state store itself.

export type TabId = string

export interface TabBase {
  id: TabId
  /** Canonical absolute folder this tab belongs to; drives sidebar/status/commands when active. */
  folderPath: string
  locked: boolean
}

export type SessionStatus = 'dormant' | 'starting' | 'running' | 'exited' | 'failed'

export interface SessionTab extends TabBase {
  kind: 'session'
  title: string
  providerId: string
  /**
   * Provider-owned conversation identity (Claude session uuid, Codex thread
   * id, Antigravity conversation id, OMP session id); null until known.
   */
  resumeToken: string | null
  status: SessionStatus
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

export interface DiffTab extends TabBase {
  kind: 'diff'
  source: DiffSource
  initialFile: string | null
  temporary: boolean
}

export interface TextTab extends TabBase {
  kind: 'text'
  filePath: string | null
  fileName: string
  temporary: boolean
  gitRef?: string
  refLabel?: string
}

export interface ToolTab extends TabBase {
  kind: 'tool'
  tool: 'notification-test'
  label: string
  temporary: boolean
}

export interface LauncherTab extends TabBase {
  kind: 'launcher'
  temporary: false
}

export type TaskRunStatus = 'starting' | 'running' | 'exited' | 'failed'

export interface TaskTab extends TabBase {
  kind: 'task'
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
}

export interface IssueTab extends TabBase {
  kind: 'issue'
  issueId: string
  /** Cached title for tab label; refreshed when surface loads detail. */
  title: string
  temporary: boolean
}

export interface EpicTab extends TabBase {
  kind: 'epic'
  epicId: string
  /** Cached title for tab label; refreshed when surface loads detail. */
  title: string
  temporary: boolean
}

export type Tab = SessionTab | DiffTab | TextTab | ToolTab | LauncherTab | TaskTab | IssueTab | EpicTab

export interface Workbench {
  tabs: Tab[]
  activeTabId: TabId | null
}

export type PersistedTab = { folderPath: string } & (
  | {
      kind: 'session'
      title: string
      providerId: string
      resumeToken: string | null
      locked: boolean
    }
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
  | {
      kind: 'launcher'
      locked: boolean
    }
  | {
      kind: 'issue'
      issueId: string
      title: string
      temporary: boolean
      locked: boolean
    }
  | {
      kind: 'epic'
      epicId: string
      title: string
      temporary: boolean
      locked: boolean
    }
)

export interface PersistedWorkbenchV4 {
  version: 4
  activeTabIndex: number
  tabs: PersistedTab[]
}

export function canLockTab(tab: Tab): boolean {
  return (
    tab.kind === 'session' || tab.kind === 'text' || tab.kind === 'task' || tab.kind === 'issue' || tab.kind === 'epic'
  )
}

/** True while a session/task tab owns a live process (spawning or running). */
export function isProcessLive(status: SessionStatus | TaskRunStatus): boolean {
  return status === 'running' || status === 'starting'
}
