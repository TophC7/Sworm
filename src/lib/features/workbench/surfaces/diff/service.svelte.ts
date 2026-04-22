import {
  addChangesTab,
  addCommitTab,
  addStashTab,
  openProject,
  type DiffTab,
  type TabId
} from '$lib/features/workbench/state.svelte'
import {
  openTextFile,
  openTextSnapshot,
  type OpenTextOptions
} from '$lib/features/workbench/surfaces/text/service.svelte'

export interface OpenDiffOptions {
  temporary?: boolean
}

export function openWorkingTreeDiff(
  projectId: string,
  staged: boolean,
  scopePath: string | null = null,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): TabId {
  openProject(projectId)
  return addChangesTab(projectId, staged, scopePath, initialFile, options.temporary ?? true)
}

export function openCommitDiff(
  projectId: string,
  commitHash: string,
  shortHash: string,
  message: string,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): TabId {
  openProject(projectId)
  return addCommitTab(projectId, commitHash, shortHash, message, initialFile, options.temporary ?? true)
}

export function openStashDiff(
  projectId: string,
  stashIndex: number,
  message: string,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): TabId {
  openProject(projectId)
  return addStashTab(projectId, stashIndex, message, initialFile, options.temporary ?? true)
}

export function openCurrentFileFromDiff(projectId: string, filePath: string, options: OpenTextOptions = {}): TabId {
  return openTextFile(projectId, filePath, options)
}

export function openCommitSnapshot(projectId: string, filePath: string, commitHash: string): TabId {
  const short = commitHash.slice(0, 7)
  return openTextSnapshot(projectId, filePath, commitHash, short)
}

export function openStashSnapshot(projectId: string, filePath: string, stashIndex: number): TabId {
  const stashRef = `stash@{${stashIndex}}`
  return openTextSnapshot(projectId, filePath, stashRef, `stash-${stashIndex}`)
}

export function openHeadSnapshot(projectId: string, filePath: string): TabId {
  return openTextSnapshot(projectId, filePath, 'HEAD', 'HEAD')
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
