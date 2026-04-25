import { backend } from '$lib/api/backend'
import { refreshGit } from '$lib/features/git/state.svelte'
import {
  lineChangesOutsideRanges,
  selectedLineChanges,
  type LineChange,
  type LineSelectionRange
} from '$lib/features/git/lineChanges'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import {
  indexContentForStatus,
  runDiffGitLineAction,
  titleForDiffGitLineAction,
  type DiffGitLineAction
} from '$lib/features/workbench/surfaces/diff/diffGitLineActions'
import type { DiffModelEntry, DiffModelStore } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'
import type { GitStatusKind } from '$lib/types/backend'
import { Codicon } from 'monaco-editor/esm/vs/base/common/codicons.js'
import { MenuId, MenuRegistry } from 'monaco-editor/esm/vs/platform/actions/common/actions.js'
import { ContextKeyExpr } from 'monaco-editor/esm/vs/platform/contextkey/common/contextkey.js'

type Monaco = typeof import('monaco-editor')
type IDisposable = { dispose(): void }
type DiffToolbarScope = 'hunk' | 'selection'

export const SWORM_DIFF_GIT_ACTIONS_CONTEXT = 'swormDiffGitActions'
export const SWORM_DIFF_STAGED_CONTEXT = 'swormDiffStaged'

interface DiffGutterRegistration {
  projectId: string
  projectPath: string
  filePath: string
  status: GitStatusKind
  staged: boolean
  store: DiffModelStore
  getLineChanges: () => readonly LineChange[]
}

interface MonacoLineRange {
  readonly startLineNumber: number
  readonly endLineNumberExclusive: number
}

interface MonacoDiffToolbarContext {
  readonly mapping?: {
    readonly original?: MonacoLineRange
    readonly modified?: MonacoLineRange
  }
  readonly originalWithModifiedChanges?: string
  readonly modifiedUri?: { toString(): string }
}

declare global {
  // eslint-disable-next-line no-var
  var __swormDiffGutterMenuRegistration: IDisposable | undefined
}

const registrations = new Map<string, DiffGutterRegistration>()

function getRegistration(context: MonacoDiffToolbarContext | undefined): DiffGutterRegistration | null {
  const uri = context?.modifiedUri?.toString()
  return uri ? (registrations.get(uri) ?? null) : null
}

function lineRangeToSelectionRange(range: MonacoLineRange | undefined, lineCount: number): LineSelectionRange | null {
  if (!range) return null
  if (range.startLineNumber === range.endLineNumberExclusive) {
    const anchor = Math.min(Math.max(1, range.startLineNumber || 1), Math.max(1, lineCount))
    return { startLineNumber: anchor, endLineNumber: anchor }
  }
  return {
    startLineNumber: Math.max(1, range.startLineNumber),
    endLineNumber: Math.max(range.startLineNumber, range.endLineNumberExclusive - 1)
  }
}

function lineChangeFromMapping(context: MonacoDiffToolbarContext | undefined): LineChange | null {
  const original = context?.mapping?.original
  const modified = context?.mapping?.modified
  if (!original || !modified) return null

  const originalEmpty = original.startLineNumber === original.endLineNumberExclusive
  const modifiedEmpty = modified.startLineNumber === modified.endLineNumberExclusive

  return {
    originalStartLineNumber: originalEmpty ? original.startLineNumber - 1 : original.startLineNumber,
    originalEndLineNumber: originalEmpty ? 0 : original.endLineNumberExclusive - 1,
    modifiedStartLineNumber: modifiedEmpty ? modified.startLineNumber - 1 : modified.startLineNumber,
    modifiedEndLineNumber: modifiedEmpty ? 0 : modified.endLineNumberExclusive - 1
  }
}

function selectedRangesFromMapping(
  context: MonacoDiffToolbarContext | undefined,
  entry: DiffModelEntry
): LineSelectionRange[] {
  const range = lineRangeToSelectionRange(context?.mapping?.modified, entry.modified?.getLineCount() ?? 1)
  return range ? [range] : []
}

async function stageToolbarContent(
  registration: DiffGutterRegistration,
  entry: DiffModelEntry,
  content: string
): Promise<void> {
  await backend.git.stageFileContent(
    registration.projectPath,
    registration.filePath,
    indexContentForStatus(registration.status, content)
  )
  await refreshGit(registration.projectId, registration.projectPath)
}

async function runToolbarAction(
  action: DiffGitLineAction,
  context: MonacoDiffToolbarContext | undefined,
  scope: DiffToolbarScope
): Promise<void> {
  const registration = getRegistration(context)
  if (!registration) return

  const entry = registration.store.get(registration.filePath)
  if (!entry) return

  try {
    if (action === 'stage' && typeof context?.originalWithModifiedChanges === 'string') {
      await stageToolbarContent(registration, entry, context.originalWithModifiedChanges)
      return
    }

    const allChanges = registration.getLineChanges()
    const ranges = selectedRangesFromMapping(context, entry)

    if (action === 'revert') {
      if (ranges.length === 0) return
      const remaining = lineChangesOutsideRanges(allChanges, ranges, entry.modified?.getLineCount() ?? 1, {
        originalContent: entry.originalContent,
        modifiedContent: entry.modifiedContent
      })
      await runDiffGitLineAction(registration, entry, action, remaining)
      return
    }

    const selected =
      ranges.length > 0
        ? selectedLineChanges(allChanges, ranges, entry.modified?.getLineCount() ?? 1, {
            originalContent: entry.originalContent,
            modifiedContent: entry.modifiedContent
          })
        : []
    const fallback = scope === 'hunk' ? lineChangeFromMapping(context) : null
    await runDiffGitLineAction(registration, entry, action, selected.length > 0 ? selected : fallback ? [fallback] : [])
  } catch (error) {
    notify.error(`${titleForDiffGitLineAction(action)} failed`, getErrorMessage(error))
  }
}

function appendGutterMenuItems(): IDisposable[] {
  const hasActions = ContextKeyExpr.has(SWORM_DIFF_GIT_ACTIONS_CONTEXT)
  const isStaged = ContextKeyExpr.equals(SWORM_DIFF_STAGED_CONTEXT, true)
  const isUnstaged = ContextKeyExpr.and(hasActions, isStaged.negate())
  const staged = ContextKeyExpr.and(hasActions, isStaged)

  return [
    MenuRegistry.appendMenuItem(MenuId.DiffEditorHunkToolbar, {
      command: {
        id: 'sworm.diff.stageHunk',
        title: 'Stage Block',
        icon: Codicon.add
      },
      when: isUnstaged,
      group: 'primary@10'
    }),
    MenuRegistry.appendMenuItem(MenuId.DiffEditorSelectionToolbar, {
      command: {
        id: 'sworm.diff.stageSelection',
        title: 'Stage Selection',
        icon: Codicon.add
      },
      when: isUnstaged,
      group: 'primary@10'
    }),
    MenuRegistry.appendMenuItem(MenuId.DiffEditorHunkToolbar, {
      command: {
        id: 'sworm.diff.revertHunk',
        title: 'Revert Block',
        icon: Codicon.discard
      },
      when: isUnstaged,
      group: 'primary@20'
    }),
    MenuRegistry.appendMenuItem(MenuId.DiffEditorSelectionToolbar, {
      command: {
        id: 'sworm.diff.revertSelection',
        title: 'Revert Selection',
        icon: Codicon.discard
      },
      when: isUnstaged,
      group: 'primary@20'
    }),
    MenuRegistry.appendMenuItem(MenuId.DiffEditorHunkToolbar, {
      command: {
        id: 'sworm.diff.unstageHunk',
        title: 'Unstage Block',
        icon: Codicon.remove
      },
      when: staged,
      group: 'primary@10'
    }),
    MenuRegistry.appendMenuItem(MenuId.DiffEditorSelectionToolbar, {
      command: {
        id: 'sworm.diff.unstageSelection',
        title: 'Unstage Selection',
        icon: Codicon.remove
      },
      when: staged,
      group: 'primary@10'
    })
  ]
}

export function ensureSwormDiffGutterMenu(monaco: Monaco): void {
  if (globalThis.__swormDiffGutterMenuRegistration) return

  const disposables: IDisposable[] = [
    monaco.editor.registerCommand('sworm.diff.stageHunk', (_accessor, context) =>
      runToolbarAction('stage', context as MonacoDiffToolbarContext | undefined, 'hunk')
    ),
    monaco.editor.registerCommand('sworm.diff.stageSelection', (_accessor, context) =>
      runToolbarAction('stage', context as MonacoDiffToolbarContext | undefined, 'selection')
    ),
    monaco.editor.registerCommand('sworm.diff.revertHunk', (_accessor, context) =>
      runToolbarAction('revert', context as MonacoDiffToolbarContext | undefined, 'hunk')
    ),
    monaco.editor.registerCommand('sworm.diff.revertSelection', (_accessor, context) =>
      runToolbarAction('revert', context as MonacoDiffToolbarContext | undefined, 'selection')
    ),
    monaco.editor.registerCommand('sworm.diff.unstageHunk', (_accessor, context) =>
      runToolbarAction('unstage', context as MonacoDiffToolbarContext | undefined, 'hunk')
    ),
    monaco.editor.registerCommand('sworm.diff.unstageSelection', (_accessor, context) =>
      runToolbarAction('unstage', context as MonacoDiffToolbarContext | undefined, 'selection')
    ),
    ...appendGutterMenuItems()
  ]

  globalThis.__swormDiffGutterMenuRegistration = {
    dispose: () => {
      for (const disposable of disposables) disposable.dispose()
    }
  }
}

export function registerSwormDiffGutterContext(modifiedUri: string, registration: DiffGutterRegistration): IDisposable {
  registrations.set(modifiedUri, registration)
  return {
    dispose: () => {
      if (registrations.get(modifiedUri) === registration) {
        registrations.delete(modifiedUri)
      }
    }
  }
}

if (import.meta.hot) {
  import.meta.hot.dispose(() => {
    globalThis.__swormDiffGutterMenuRegistration?.dispose()
    globalThis.__swormDiffGutterMenuRegistration = undefined
    registrations.clear()
  })
}
