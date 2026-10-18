import { backend } from '$lib/api/backend'
import { setSidebarCollapsed, setSidebarView } from '$lib/features/app-shell/sidebar/state.svelte'
import { confirmAsync } from '$lib/features/confirm/service.svelte'
import { revealInExplorer } from '$lib/features/files/explorer.svelte'
import { discardAllFolder } from '$lib/features/git/actions.svelte'
import { discardFiles, stageFiles, unstageFiles } from '$lib/features/git/state.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { getActivationSeq } from '$lib/features/workbench/state.svelte'
import { openHeadSnapshot } from '$lib/features/workbench/surfaces/diff/service.svelte'
import { openTextFile } from '$lib/features/workbench/surfaces/text/service.svelte'
import { platform } from '$lib/platform'
import type { GitChange, GitConflict } from '$lib/types/backend'
import { getErrorMessage } from '$lib/utils/client-error'
import { resolveProjectFile, splitRemotePath } from '$lib/utils/paths'

export type GitTreeSection = 'staged' | 'changes' | 'merge'
export type GitTreeMenuTarget =
  | { kind: 'conflict'; conflict: GitConflict }
  | { kind: 'file'; section: GitTreeSection; change: GitChange }
  | { kind: 'folder'; section: GitTreeSection; path: string; files: string[] }
  | { kind: 'section'; section: GitTreeSection; files: string[] }

export async function runPathsAction(
  folderPath: string,
  action: 'stage' | 'unstage' | 'discard',
  files: string[],
  description: string
): Promise<void> {
  if (files.length === 0) return
  try {
    switch (action) {
      case 'stage':
        if (!(await stageFiles(folderPath, files))) return
        break
      case 'unstage':
        await unstageFiles(folderPath, files)
        break
      case 'discard':
        if (!(await discardFiles(folderPath, files))) return
        break
    }
    notify.success(
      action === 'discard' ? 'Changes reverted' : action === 'stage' ? 'Changes staged' : 'Changes unstaged',
      description
    )
  } catch (e) {
    notify.error(
      action === 'discard' ? 'Revert failed' : action === 'stage' ? 'Stage failed' : 'Unstage failed',
      getErrorMessage(e)
    )
  }
}

export async function confirmDiscardPaths(folderPath: string, files: string[], description: string): Promise<void> {
  if (files.length === 0) return
  const proceed = await confirmAsync({
    title: 'Revert Changes?',
    message: `Revert unstaged changes for ${description}? Tracked edits cannot be recovered; untracked files move to the trash.`,
    confirmLabel: 'Revert'
  })
  if (!proceed) return
  await runPathsAction(folderPath, 'discard', files, description)
}

export async function confirmDiscardAll(folderPath: string): Promise<void> {
  const proceed = await confirmAsync({
    title: 'Discard All Changes?',
    message: 'Discard all unstaged changes? Tracked edits cannot be recovered; untracked files move to the trash.',
    confirmLabel: 'Discard All'
  })
  if (!proceed) return
  await discardAllFolder(folderPath)
}

export function describeTarget(target: GitTreeMenuTarget): string {
  if (target.kind === 'file') return target.change.path
  if (target.kind === 'conflict') return target.conflict.path
  const count = `${target.files.length} file${target.files.length === 1 ? '' : 's'}`
  return target.kind === 'folder' ? `${count} under ${target.path}` : count
}

export async function copyPatch(folderPath: string, files: string[], staged: boolean, label: string): Promise<void> {
  try {
    const patch = files.length > 0 ? await backend.git.getPathPatch(folderPath, files, staged) : null
    if (patch) await platform.clipboard.writeText(patch)
    else notify.info('No diff to copy', `No ${staged ? 'staged' : 'unstaged'} changes in ${label}.`)
  } catch (e) {
    notify.error('Copy patch failed', getErrorMessage(e))
  }
}

export async function copyFullPatch(folderPath: string): Promise<void> {
  try {
    const patch = await backend.git.getFullPatch(folderPath)
    if (patch) await platform.clipboard.writeText(patch)
    else notify.info('No diff to copy', 'No changes in the working tree.')
  } catch (e) {
    notify.error('Copy patch failed', getErrorMessage(e))
  }
}

export async function copyPath(folderPath: string, path: string): Promise<void> {
  try {
    const absPath = resolveProjectFile(folderPath, path)
    await platform.clipboard.writeText(splitRemotePath(absPath)?.path ?? absPath)
  } catch (error) {
    notify.error('Copy path failed', getErrorMessage(error))
  }
}

export async function copyRelativePath(path: string): Promise<void> {
  try {
    await platform.clipboard.writeText(path)
  } catch (error) {
    notify.error('Copy relative path failed', getErrorMessage(error))
  }
}

export function canRevealInFileManager(folderPath: string): boolean {
  return !splitRemotePath(folderPath) && platform.native !== null
}

export async function revealInFileManager(folderPath: string, path: string): Promise<void> {
  const native = platform.native
  if (splitRemotePath(folderPath) || !native) return
  try {
    await native.files.reveal(resolveProjectFile(folderPath, path))
  } catch (error) {
    notify.error('Reveal file failed', getErrorMessage(error))
  }
}

export function revealInExplorerView(folderPath: string, path: string): void {
  setSidebarCollapsed(false)
  setSidebarView('files')
  void revealInExplorer(folderPath, path, getActivationSeq()).catch((error) =>
    notify.error('Reveal in Explorer failed', getErrorMessage(error))
  )
}

export async function openActualFile(folderPath: string, path: string): Promise<void> {
  try {
    await openTextFile(folderPath, path, { temporary: false })
  } catch (e) {
    notify.error('Open file failed', getErrorMessage(e))
  }
}

export async function openHeadFile(folderPath: string, path: string): Promise<void> {
  try {
    await openHeadSnapshot(folderPath, path)
  } catch (e) {
    notify.error('Open file (HEAD) failed', getErrorMessage(e))
  }
}

export function canOpenHead(change: GitChange): boolean {
  return change.status === 'M' || change.status === 'D'
}
