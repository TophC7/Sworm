import { backend } from '$lib/api/backend'
import { refreshGit } from '$lib/features/git/state.svelte'
import { applyLineChanges, compareLineChanges, invertLineChange, type LineChange } from '$lib/features/git/lineChanges'
import type { DiffModelEntry } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'
import type { GitStatusKind } from '$lib/types/backend'

export type DiffGitLineAction = 'stage' | 'unstage' | 'revert'

export interface DiffGitLineActionContext {
  projectId: string
  projectPath: string
  filePath: string
  status: GitStatusKind
}

export function indexContentForStatus(status: GitStatusKind, content: string): string | null {
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

export async function runDiffGitLineAction(
  context: DiffGitLineActionContext,
  entry: DiffModelEntry,
  action: DiffGitLineAction,
  changes: readonly LineChange[]
): Promise<void> {
  if (changes.length === 0 && action !== 'revert') return

  const originalContent = entry.originalContent
  const modifiedContent = entry.modifiedContent

  if (action === 'stage') {
    const nextIndex = applyLineChanges(originalContent, modifiedContent, changes)
    await backend.git.stageFileContent(
      context.projectPath,
      context.filePath,
      indexContentForStatus(context.status, nextIndex)
    )
  } else if (action === 'unstage') {
    const inverted = changes.map(invertLineChange).sort(compareLineChanges)
    const nextIndex = applyLineChanges(modifiedContent, originalContent, inverted)
    await backend.git.stageFileContent(
      context.projectPath,
      context.filePath,
      indexContentForStatus(context.status, nextIndex)
    )
  } else {
    const nextWorkingTree = applyLineChanges(originalContent, modifiedContent, changes)
    if (context.status === 'untracked' && nextWorkingTree.length === 0) {
      await backend.files.delete(context.projectPath, context.filePath)
    } else {
      await backend.files.write(context.projectPath, context.filePath, nextWorkingTree)
    }
  }

  await refreshGit(context.projectId, context.projectPath)
}
