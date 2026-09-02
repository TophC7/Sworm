import type { DiffTab, TabId } from '$lib/features/workbench/model'
import { addChangesTab, addCommitTab, addStashTab } from '$lib/features/workbench/state.svelte'
import {
  openTextFile,
  openTextSnapshot,
  type OpenTextOptions
} from '$lib/features/workbench/surfaces/text/service.svelte'

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

export function openCurrentFileFromDiff(
  folderPath: string,
  filePath: string,
  options: OpenTextOptions = {}
): Promise<TabId> {
  return openTextFile(folderPath, filePath, options)
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
    default: {
      const _exhaustive: never = tab.source
      return _exhaustive
    }
  }
}
