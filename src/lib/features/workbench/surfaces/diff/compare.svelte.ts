import { pickFileAsync } from '$lib/features/command-palette/state.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import type { Tab } from '$lib/features/workbench/model'
import { getActiveTab } from '$lib/features/workbench/state.svelte'
import { platform } from '$lib/platform'
import { getErrorMessage } from '$lib/utils/client-error'
import { basename } from '$lib/utils/paths'
import { openCompareDiff } from './service.svelte'

/** A saved file a compare action starts from. */
export interface CompareFile {
  folderPath: string
  path: string
}

let selected = $state<CompareFile | null>(null)

export function selectForCompare(file: CompareFile): void {
  selected = file
}

export function canCompareWithSelected(file: CompareFile): boolean {
  // Same folder only: the tab's folder owns both sides' claims, watches and persisted paths.
  return selected !== null && selected.folderPath === file.folderPath && selected.path !== file.path
}

export function compareWithSelected(file: CompareFile): void {
  if (selected && canCompareWithSelected(file)) {
    openCompareDiff(file.folderPath, { kind: 'file', path: selected.path }, { kind: 'file', path: file.path })
  }
}

export async function compareWithClipboard(file: CompareFile): Promise<void> {
  let content: string
  try {
    content = (await platform.clipboard.readText()) ?? ''
  } catch (e) {
    notify.error('Compare with Clipboard failed', getErrorMessage(e))
    return
  }
  openCompareDiff(file.folderPath, { kind: 'clipboard', content }, { kind: 'file', path: file.path })
}

export async function compareFileWith(file: CompareFile): Promise<void> {
  const path = await pickFileAsync(file.folderPath, `Compare ${basename(file.path)} with…`)
  if (path === null) return
  openCompareDiff(file.folderPath, { kind: 'file', path: file.path }, { kind: 'file', path })
}

/** The active tab as a compare file: a saved-path text tab that is not a git snapshot. */
export function activeCompareFile(): CompareFile | null {
  const tab = getActiveTab()
  return tab ? compareFileOf(tab) : null
}

export function compareFileOf(tab: Tab): CompareFile | null {
  return tab.kind === 'text' && tab.filePath !== null && !tab.gitRef
    ? { folderPath: tab.folderPath, path: tab.filePath }
    : null
}
