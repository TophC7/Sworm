import { backend } from '$lib/api/backend'
import { runGitAction } from '$lib/features/git/state.svelte'
import {
  applyLineChanges,
  compareLineChanges,
  invertLineChange,
  lineChangesOutsideRanges,
  selectedLineChanges,
  type LineChange,
  type LineChangeContent,
  type LineSelectionRange
} from '$lib/features/git/lineChanges'
import type { GitStatusKind } from '$lib/types/backend'

export type DiffGitLineAction = 'stage' | 'unstage' | 'revert'

export interface DiffGitLineActionContext {
  folderPath: string
  filePath: string
  status: GitStatusKind
}

function indexContentForStatus(status: GitStatusKind, content: string): string | null {
  if ((status === 'added' || status === 'deleted') && content.length === 0) return null
  return content
}

export function titleForDiffGitLineAction(action: DiffGitLineAction): string {
  switch (action) {
    case 'stage':
      return 'Stage'
    case 'unstage':
      return 'Unstage'
    case 'revert':
      return 'Revert'
  }
}

export async function stageDiffIndexContent(context: DiffGitLineActionContext, content: string): Promise<void> {
  await runGitAction(
    context.folderPath,
    (path) => backend.git.stageFileContent(path, context.filePath, indexContentForStatus(context.status, content)),
    { scope: 'summary' }
  )
}

export function diffLineChangesForAction(
  action: DiffGitLineAction,
  changes: readonly LineChange[],
  ranges: readonly LineSelectionRange[],
  modifiedLineCount: number,
  content?: LineChangeContent
): LineChange[] {
  return action === 'revert'
    ? lineChangesOutsideRanges(changes, ranges, modifiedLineCount, content)
    : selectedLineChanges(changes, ranges, modifiedLineCount, content)
}

export async function runDiffGitLineAction(
  context: DiffGitLineActionContext,
  content: LineChangeContent,
  action: DiffGitLineAction,
  changes: readonly LineChange[]
): Promise<void> {
  if (changes.length === 0 && action !== 'revert') return

  const { originalContent, modifiedContent } = content
  if (action === 'stage') {
    await stageDiffIndexContent(context, applyLineChanges(originalContent, modifiedContent, changes))
  } else if (action === 'unstage') {
    const inverted = changes.map(invertLineChange).sort(compareLineChanges)
    await stageDiffIndexContent(context, applyLineChanges(modifiedContent, originalContent, inverted))
  } else {
    const nextWorkingTree = applyLineChanges(originalContent, modifiedContent, changes)
    await runGitAction(
      context.folderPath,
      async (path) => {
        if (context.status === 'untracked' && nextWorkingTree.length === 0) {
          await backend.files.delete(path, context.filePath)
        } else {
          await backend.files.write(path, context.filePath, nextWorkingTree)
        }
      },
      { scope: 'summary' }
    )
  }
}
