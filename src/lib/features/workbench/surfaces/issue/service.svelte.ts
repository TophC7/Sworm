import type { IssueTab, TabId } from '$lib/features/workbench/model'
import { addIssueTab } from '$lib/features/workbench/state.svelte'

/** Open or focus an issue tab in its owning folder. */
export function openIssueTab(
  folderPath: string,
  issueId: string,
  title: string,
  options: { temporary?: boolean } = {}
): TabId {
  return addIssueTab(folderPath, issueId, title, options.temporary ?? true)
}

export function getIssueTabTitle(tab: IssueTab): string {
  return `${tab.issueId}: ${tab.title}`
}
