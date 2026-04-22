import type { DiffTab, TabId } from '$lib/features/workbench/model'
import {
  addChangesTab,
  addCommitTab,
  addStashTab,
  openProject,
  restoreWorkspaceFromDisk
} from '$lib/features/workbench/state.svelte'
import {
  openTextFile,
  openTextSnapshot,
  type OpenTextOptions
} from '$lib/features/workbench/surfaces/text/service.svelte'

export interface OpenDiffOptions {
  temporary?: boolean
}

async function ensureProjectWorkspaceReady(projectId: string): Promise<void> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
}

export async function openWorkingTreeDiff(
  projectId: string,
  staged: boolean,
  scopePath: string | null = null,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): Promise<TabId> {
  await ensureProjectWorkspaceReady(projectId)
  return addChangesTab(projectId, staged, scopePath, initialFile, options.temporary ?? true)
}

export async function openCommitDiff(
  projectId: string,
  commitHash: string,
  shortHash: string,
  message: string,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): Promise<TabId> {
  await ensureProjectWorkspaceReady(projectId)
  return addCommitTab(projectId, commitHash, shortHash, message, initialFile, options.temporary ?? true)
}

export async function openStashDiff(
  projectId: string,
  stashIndex: number,
  message: string,
  initialFile: string | null = null,
  options: OpenDiffOptions = {}
): Promise<TabId> {
  await ensureProjectWorkspaceReady(projectId)
  return addStashTab(projectId, stashIndex, message, initialFile, options.temporary ?? true)
}

export function openCurrentFileFromDiff(
  projectId: string,
  filePath: string,
  options: OpenTextOptions = {}
): Promise<TabId> {
  return openTextFile(projectId, filePath, options)
}

export function openCommitSnapshot(projectId: string, filePath: string, commitHash: string): Promise<TabId> {
  const short = commitHash.slice(0, 7)
  return openTextSnapshot(projectId, filePath, commitHash, short)
}

export function openStashSnapshot(projectId: string, filePath: string, stashIndex: number): Promise<TabId> {
  const stashRef = `stash@{${stashIndex}}`
  return openTextSnapshot(projectId, filePath, stashRef, `stash-${stashIndex}`)
}

export function openHeadSnapshot(projectId: string, filePath: string): Promise<TabId> {
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
