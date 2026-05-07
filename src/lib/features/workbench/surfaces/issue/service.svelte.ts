import type { IssueTab, TabId } from '$lib/features/workbench/model'
import { addIssueTab, openProject, promoteTab, restoreWorkspaceFromDisk } from '$lib/features/workbench/state.svelte'

/**
 * Open (or focus) an issue surface tab. Mirrors the diff/text surface
 * service shape: ensures the project is hydrated, then routes through
 * the workbench's content-tab dedup so re-clicking the same issue
 * focuses the existing tab instead of stacking duplicates.
 */
export async function openIssueTab(
  projectId: string,
  issueId: string,
  title: string,
  options: { temporary?: boolean } = {}
): Promise<TabId> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
  return addIssueTab(projectId, issueId, title, options.temporary ?? true)
}

export function promoteIssueTab(projectId: string, tabId: TabId): void {
  promoteTab(projectId, tabId)
}

export function getIssueTabTitle(tab: IssueTab): string {
  return `${tab.issueId}: ${tab.title}`
}
