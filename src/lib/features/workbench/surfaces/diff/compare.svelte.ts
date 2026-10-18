import { pickFileAsync } from '$lib/features/command-palette/state.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import { compareSidesEqual, type CompareFileSide, type Tab } from '$lib/features/workbench/model'
import { getActiveTab } from '$lib/features/workbench/state.svelte'
import { platform } from '$lib/platform'
import { getErrorMessage } from '$lib/utils/client-error'
import { basename } from '$lib/utils/paths'
import { openCompareDiff } from './service.svelte'

let selected = $state<CompareFileSide | null>(null)

export function selectForCompare(file: CompareFileSide): void {
  selected = file
}

export function canCompareWithSelected(file: CompareFileSide): boolean {
  return selected !== null && !compareSidesEqual(selected, file)
}

export function compareWithSelected(file: CompareFileSide): void {
  if (selected && canCompareWithSelected(file)) openCompareDiff(selected, file)
}

export async function compareWithClipboard(file: CompareFileSide): Promise<void> {
  let content: string
  try {
    content = (await platform.clipboard.readText()) ?? ''
  } catch (e) {
    notify.error('Compare with Clipboard failed', getErrorMessage(e))
    return
  }
  openCompareDiff({ kind: 'clipboard', content }, file)
}

export async function compareFileWith(file: CompareFileSide): Promise<void> {
  const path = await pickFileAsync(file.folderPath, `Compare ${basename(file.path)} with…`)
  if (path === null) return
  openCompareDiff(file, { kind: 'file', folderPath: file.folderPath, path })
}

/** The active tab as a compare side: a saved-path text tab that is not a git snapshot. */
export function activeCompareFile(): CompareFileSide | null {
  const tab = getActiveTab()
  return tab ? compareFileOf(tab) : null
}

export function compareFileOf(tab: Tab): CompareFileSide | null {
  return tab.kind === 'text' && tab.filePath !== null && !tab.gitRef
    ? { kind: 'file', folderPath: tab.folderPath, path: tab.filePath }
    : null
}
