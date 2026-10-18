import { backend } from '$lib/api/backend'
import { notify } from '$lib/features/notifications/state.svelte'
import type { DiffTab, TabId } from '$lib/features/workbench/model'
import { addChangesTab, addCommitTab, addRangeTab, addStashTab } from '$lib/features/workbench/state.svelte'
import { openTextSnapshot } from '$lib/features/workbench/surfaces/text/service.svelte'
import { getErrorMessage } from '$lib/utils/client-error'

export interface OpenDiffOptions {
  temporary?: boolean
}

export async function openWorkingTreeDiff(
  folderPath: string,
  staged: boolean,
  scopePath: string | null = null,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): Promise<TabId> {
  return addChangesTab(folderPath, staged, scopePath, initialFile, options.temporary ?? true)
}

export async function openCommitDiff(
  folderPath: string,
  commitHash: string,
  shortHash: string,
  message: string,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): Promise<TabId> {
  return addCommitTab(folderPath, commitHash, shortHash, message, initialFile, options.temporary ?? true)
}

export async function openStashDiff(
  folderPath: string,
  stashIndex: number,
  message: string,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): Promise<TabId> {
  return addStashTab(folderPath, stashIndex, message, initialFile, options.temporary ?? true)
}

export async function openRangeDiff(
  folderPath: string,
  base: { rev: string; label: string },
  target: { rev: string; label: string } | null,
  options: OpenDiffOptions = {}
): Promise<TabId | null> {
  try {
    const hashes = await Promise.all(
      [base.rev, ...(target ? [target.rev] : [])].map(async (rev) => {
        if (/^[0-9a-f]{40}$/i.test(rev)) return rev
        const hash = (await backend.git.getBranchCommits(folderPath, rev, 1))[0]?.hash
        if (!hash) throw new Error(`Could not resolve ${rev}`)
        return hash
      })
    )
    return addRangeTab(
      folderPath,
      hashes[0],
      base.label,
      target ? hashes[1] : null,
      target?.label ?? 'Working Tree',
      options.temporary ?? true
    )
  } catch (error) {
    notify.error('Compare failed', getErrorMessage(error))
    return null
  }
}

export function openCommitSnapshot(folderPath: string, filePath: string, commitHash: string): Promise<TabId> {
  const short = commitHash.slice(0, 7)
  return openTextSnapshot(folderPath, filePath, commitHash, short)
}

export function openStashSnapshot(folderPath: string, filePath: string, stashIndex: number): Promise<TabId> {
  const stashRef = `stash@{${stashIndex}}`
  return openTextSnapshot(folderPath, filePath, stashRef, `stash-${stashIndex}`)
}

export function openHeadSnapshot(folderPath: string, filePath: string): Promise<TabId> {
  return openTextSnapshot(folderPath, filePath, 'HEAD', 'HEAD')
}

export function getDiffTabTitle(tab: DiffTab): string {
  switch (tab.source.kind) {
    case 'working':
      return tab.source.scopePath
        ? `Changes: ${tab.source.scopePath}`
        : tab.source.staged
          ? 'Staged Changes'
          : 'Changes'
    case 'commit':
      return tab.source.shortHash
    case 'stash':
      return `stash@{${tab.source.stashIndex}}`
    case 'range':
      return `${tab.source.baseLabel} ↔ ${tab.source.targetLabel}`
    default: {
      const _exhaustive: never = tab.source
      return _exhaustive
    }
  }
}
