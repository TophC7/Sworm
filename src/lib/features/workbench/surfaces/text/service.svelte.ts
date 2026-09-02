import {
  discardTextModelBuffer,
  discardUntitledTextModelBuffer,
  markTextModelBufferSaved,
  renameTextModelBuffer
} from '$lib/features/editor/renderers/monaco/text/modelCache'
import type { Tab, TabId, TextTab } from '$lib/features/workbench/model'
import {
  addReadonlyTextTab,
  addTextTab,
  addUntitledTextTab,
  closeTab,
  getTabs,
  renameTextTab,
  setActiveTab
} from '$lib/features/workbench/state.svelte'

export type TextRevealTarget =
  | {
      kind: 'range'
      startLineNumber: number
      startColumn: number
      endLineNumber: number
      endColumn: number
    }
  | {
      kind: 'position'
      lineNumber: number
      column: number
    }

export interface OpenTextOptions {
  temporary?: boolean
  reveal?: TextRevealTarget | null
}

export interface MountedTextSurfaceController {
  focus: () => void
  reveal: (target: TextRevealTarget) => void
}

const pendingReveals = new Map<TabId, TextRevealTarget>()
const mountedControllers = new Map<TabId, MountedTextSurfaceController>()

const dirtyTabs = $state<Set<TabId>>(new Set())

function isLiveTextTab(tab: Tab, folderPath: string, filePath: string): tab is TextTab {
  return tab.kind === 'text' && tab.folderPath === folderPath && tab.filePath === filePath && !tab.gitRef
}

function isSnapshotTextTab(tab: Tab, folderPath: string, filePath: string, gitRef: string): tab is TextTab {
  return tab.kind === 'text' && tab.folderPath === folderPath && tab.filePath === filePath && tab.gitRef === gitRef
}

function revealTextTab(tabId: TabId, target: TextRevealTarget) {
  const controller = mountedControllers.get(tabId)
  if (controller) {
    controller.reveal(target)
    controller.focus()
    return
  }
  pendingReveals.set(tabId, target)
}

function forgetTab(tabId: TabId) {
  dirtyTabs.delete(tabId)
  pendingReveals.delete(tabId)
  mountedControllers.delete(tabId)
}

export function registerMountedTextSurface(tabId: TabId, controller: MountedTextSurfaceController): void {
  mountedControllers.set(tabId, controller)
}

export function unregisterMountedTextSurface(tabId: TabId, controller?: MountedTextSurfaceController): void {
  if (controller && mountedControllers.get(tabId) !== controller) return
  mountedControllers.delete(tabId)
}

export function takePendingTextReveal(tabId: TabId): TextRevealTarget | null {
  const target = pendingReveals.get(tabId) ?? null
  if (target) pendingReveals.delete(tabId)
  return target
}

export async function openTextFile(
  folderPath: string,
  filePath: string,
  options: OpenTextOptions = {}
): Promise<TabId> {
  const existing = getTabs().find((tab) => isLiveTextTab(tab, folderPath, filePath))
  if (existing) setActiveTab(existing.id)
  const tabId = existing?.id ?? addTextTab(folderPath, filePath, options.temporary ?? true)
  if (options.reveal) revealTextTab(tabId, options.reveal)
  return tabId
}

export async function openTextSnapshot(
  folderPath: string,
  filePath: string,
  gitRef: string,
  refLabel: string,
  options: OpenTextOptions = {}
): Promise<TabId> {
  const existing = getTabs().find((tab) => isSnapshotTextTab(tab, folderPath, filePath, gitRef))
  if (existing) setActiveTab(existing.id)
  const tabId = existing?.id ?? addReadonlyTextTab(folderPath, filePath, gitRef, refLabel, options.temporary ?? true)
  if (options.reveal) revealTextTab(tabId, options.reveal)
  return tabId
}

export function createUntitledTextSurface(folderPath: string): TabId {
  return addUntitledTextTab(folderPath)
}

export function setTextSurfaceDirty(tabId: TabId, dirty: boolean): void {
  if (dirty === dirtyTabs.has(tabId)) return
  if (dirty) dirtyTabs.add(tabId)
  else dirtyTabs.delete(tabId)
}

export function clearTextSurfaceDirty(tabId: TabId): void {
  setTextSurfaceDirty(tabId, false)
}

export function clearTextSurfaceDirtyIfClosed(tabId: TabId): void {
  if (getTabs().some((tab) => tab.id === tabId)) return
  clearTextSurfaceDirty(tabId)
}

export function hasAnyDirtyTextSurfaces(): boolean {
  return dirtyTabs.size > 0
}

export function getDirtyTextSurfaceCount(): number {
  return dirtyTabs.size
}

export function isTextSurfaceDirty(tabId: TabId): boolean {
  return dirtyTabs.has(tabId)
}

export function markTextSurfaceSaved(folderPath: string, filePath: string, value: string): void {
  markTextModelBufferSaved(folderPath, filePath, value)
}

export function discardTextSurfaceBuffer(tab: Pick<TextTab, 'id' | 'folderPath' | 'filePath' | 'gitRef'>): void {
  if (tab.gitRef) return
  if (tab.filePath == null) {
    discardUntitledTextModelBuffer(tab.folderPath, tab.id)
    return
  }
  discardTextModelBuffer(tab.folderPath, tab.filePath)
}

export function renameTextPath(folderPath: string, oldPath: string, newPath: string): void {
  const prefix = `${oldPath}/`

  for (const tab of getTabs()) {
    if (tab.kind !== 'text' || tab.folderPath !== folderPath || tab.filePath == null) continue

    if (tab.filePath === oldPath) {
      renameTextModelBuffer(folderPath, oldPath, newPath, `${folderPath}/${newPath}`)
      renameTextTab(tab.id, newPath)
      continue
    }

    if (tab.filePath.startsWith(prefix)) {
      const renamedPath = `${newPath}/${tab.filePath.slice(prefix.length)}`
      renameTextModelBuffer(folderPath, tab.filePath, renamedPath, `${folderPath}/${renamedPath}`)
      renameTextTab(tab.id, renamedPath)
    }
  }
}

export function deleteTextPath(folderPath: string, path: string): void {
  const prefix = `${path}/`

  for (const tab of getTabs()) {
    if (tab.kind !== 'text' || tab.folderPath !== folderPath || tab.filePath == null) continue
    if (tab.filePath === path || tab.filePath.startsWith(prefix)) {
      discardTextSurfaceBuffer(tab)
      closeTab(tab.id)
      forgetTab(tab.id)
    }
  }
}

export function getTextTabTitle(tab: TextTab): string {
  return tab.refLabel ? `${tab.fileName} (${tab.refLabel})` : tab.fileName
}

export function getTextTabFileName(tab: TextTab): string {
  return tab.fileName
}
