import { backend } from '$lib/api/backend'
import { ensureFreshSession } from '$lib/surfaces/session/service.svelte'
import {
  addReadonlyTextTab,
  addTextTab,
  addUntitledTextTab,
  closeTab,
  focusTab,
  getAllTabs,
  moveTabToPane,
  openProject,
  renameTextTab,
  type TextTab,
  type PaneSlot,
  type Tab,
  type TabId
} from '$lib/workbench/state.svelte'

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
  paneSlot?: PaneSlot
  reveal?: TextRevealTarget | null
}

interface MountedTextSurfaceController {
  focus: () => void
  reveal: (target: TextRevealTarget) => void
}

const pendingReveals = new Map<TabId, TextRevealTarget>()
const mountedControllers = new Map<TabId, MountedTextSurfaceController>()

const dirtyKeys = $state<Set<string>>(new Set())

function makeDirtyKey(projectId: string, tabId: string): string {
  return `${projectId}::${tabId}`
}

function isLiveTextTab(tab: Tab, filePath: string): tab is TextTab {
  return tab.kind === 'text' && tab.filePath === filePath && !tab.gitRef
}

function isSnapshotTextTab(tab: Tab, filePath: string, gitRef: string): tab is TextTab {
  return tab.kind === 'text' && tab.filePath === filePath && tab.gitRef === gitRef
}

function placeAndFocus(projectId: string, tabId: TabId, paneSlot?: PaneSlot) {
  if (paneSlot) {
    moveTabToPane(projectId, tabId, paneSlot)
  }
  focusTab(projectId, tabId)
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

export function registerMountedTextSurface(tabId: TabId, controller: MountedTextSurfaceController): void {
  mountedControllers.set(tabId, controller)
}

export function unregisterMountedTextSurface(tabId: TabId): void {
  mountedControllers.delete(tabId)
}

export function takePendingTextReveal(tabId: TabId): TextRevealTarget | null {
  const target = pendingReveals.get(tabId) ?? null
  if (target) pendingReveals.delete(tabId)
  return target
}

export function openTextFile(projectId: string, filePath: string, options: OpenTextOptions = {}): TabId {
  openProject(projectId)

  const existing = getAllTabs(projectId).find((tab) => isLiveTextTab(tab, filePath))
  if (existing) {
    placeAndFocus(projectId, existing.id, options.paneSlot)
    if (options.reveal) revealTextTab(existing.id, options.reveal)
    return existing.id
  }

  const tabId = addTextTab(projectId, filePath, options.temporary ?? true)
  placeAndFocus(projectId, tabId, options.paneSlot)
  if (options.reveal) revealTextTab(tabId, options.reveal)
  return tabId
}

export function openTextSnapshot(
  projectId: string,
  filePath: string,
  gitRef: string,
  refLabel: string,
  options: OpenTextOptions = {}
): TabId {
  openProject(projectId)

  const existing = getAllTabs(projectId).find((tab) => isSnapshotTextTab(tab, filePath, gitRef))
  if (existing) {
    placeAndFocus(projectId, existing.id, options.paneSlot)
    if (options.reveal) revealTextTab(existing.id, options.reveal)
    return existing.id
  }

  const tabId = addReadonlyTextTab(projectId, filePath, gitRef, refLabel, options.temporary ?? true)
  placeAndFocus(projectId, tabId, options.paneSlot)
  if (options.reveal) revealTextTab(tabId, options.reveal)
  return tabId
}

export function createUntitledTextSurface(projectId: string, paneSlot?: PaneSlot): TabId {
  openProject(projectId)
  const tabId = addUntitledTextTab(projectId)
  placeAndFocus(projectId, tabId, paneSlot)
  return tabId
}

export function setTextSurfaceDirty(projectId: string, tabId: TabId, dirty: boolean): void {
  const key = makeDirtyKey(projectId, tabId)
  const has = dirtyKeys.has(key)
  if (dirty === has) return
  if (dirty) dirtyKeys.add(key)
  else dirtyKeys.delete(key)
}

export function clearTextSurfaceDirty(projectId: string, tabId: TabId): void {
  setTextSurfaceDirty(projectId, tabId, false)
}

export function hasAnyDirtyTextSurfaces(): boolean {
  return dirtyKeys.size > 0
}

export function getDirtyTextSurfaceCount(): number {
  return dirtyKeys.size
}

export function isTextSurfaceDirty(projectId: string, tabId: TabId): boolean {
  return dirtyKeys.has(makeDirtyKey(projectId, tabId))
}

export function renameTextPath(projectId: string, oldPath: string, newPath: string): void {
  const tabs = getAllTabs(projectId)
  const prefix = `${oldPath}/`

  for (const tab of tabs) {
    if (tab.kind !== 'text' || tab.filePath == null) continue

    if (tab.filePath === oldPath) {
      renameTextTab(projectId, tab.id, newPath)
      continue
    }

    if (tab.filePath.startsWith(prefix)) {
      renameTextTab(projectId, tab.id, `${newPath}/${tab.filePath.slice(prefix.length)}`)
    }
  }
}

export function deleteTextPath(projectId: string, path: string): void {
  const tabs = getAllTabs(projectId)
  const prefix = `${path}/`

  for (const tab of tabs) {
    if (tab.kind !== 'text' || tab.filePath == null) continue
    if (tab.filePath === path || tab.filePath.startsWith(prefix)) {
      closeTab(projectId, tab.id)
      clearTextSurfaceDirty(projectId, tab.id)
      pendingReveals.delete(tab.id)
      mountedControllers.delete(tab.id)
    }
  }
}

export async function openTextInFresh(projectId: string, projectPath: string, filePath: string): Promise<void> {
  await ensureFreshSession(projectId)
  await backend.fresh.openFile(projectId, projectPath, filePath)
}

export function getTextTabTitle(tab: TextTab): string {
  return tab.refLabel ? `${tab.fileName} (${tab.refLabel})` : tab.fileName
}

export function getTextTabFileName(tab: TextTab): string {
  return tab.fileName
}
