import { backend } from '$lib/api/backend'
import {
  discardProjectTextModelBuffers,
  discardTextModelBuffer,
  discardUntitledTextModelBuffer,
  markTextModelBufferSaved,
  renameTextModelBuffer
} from '$lib/features/editor/renderers/monaco/text/modelCache'
import { ensureFreshSession } from '$lib/features/workbench/surfaces/session/service.svelte'
import type { PaneSlot, Tab, TabId, TextTab } from '$lib/features/workbench/model'
import {
  addReadonlyTextTab,
  addTextTab,
  addUntitledTextTab,
  closeTab,
  focusTab,
  getAllTabs,
  moveTabToPane,
  onProjectClosed,
  openProject,
  renameTextTab,
  restoreWorkspaceFromDisk
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
  paneSlot?: PaneSlot
  reveal?: TextRevealTarget | null
}

export interface MountedTextSurfaceController {
  focus: () => void
  reveal: (target: TextRevealTarget) => void
}

const pendingReveals = new Map<TabId, TextRevealTarget>()
const pendingRevealProjects = new Map<TabId, string>()
const mountedControllers = new Map<TabId, MountedTextSurfaceController>()
const mountedControllerProjects = new Map<TabId, string>()

const dirtyKeys = $state<Set<string>>(new Set())
onProjectClosed((projectId) => clearTextSurfaceProjectState(projectId))

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

function revealTextTab(projectId: string, tabId: TabId, target: TextRevealTarget) {
  const controller = mountedControllers.get(tabId)
  if (controller) {
    controller.reveal(target)
    controller.focus()
    return
  }
  pendingReveals.set(tabId, target)
  pendingRevealProjects.set(tabId, projectId)
}

export function registerMountedTextSurface(
  projectId: string | null,
  tabId: TabId,
  controller: MountedTextSurfaceController
): void {
  mountedControllers.set(tabId, controller)
  if (projectId) mountedControllerProjects.set(tabId, projectId)
}

export function unregisterMountedTextSurface(tabId: TabId, controller?: MountedTextSurfaceController): void {
  if (controller && mountedControllers.get(tabId) !== controller) return
  mountedControllers.delete(tabId)
  mountedControllerProjects.delete(tabId)
}

export function takePendingTextReveal(tabId: TabId): TextRevealTarget | null {
  const target = pendingReveals.get(tabId) ?? null
  if (target) {
    pendingReveals.delete(tabId)
    pendingRevealProjects.delete(tabId)
  }
  return target
}

async function ensureProjectWorkspaceReady(projectId: string): Promise<void> {
  openProject(projectId)
  await restoreWorkspaceFromDisk(projectId)
}

export async function openTextFile(projectId: string, filePath: string, options: OpenTextOptions = {}): Promise<TabId> {
  await ensureProjectWorkspaceReady(projectId)
  const existing = getAllTabs(projectId).find((tab) => isLiveTextTab(tab, filePath))
  if (existing) {
    placeAndFocus(projectId, existing.id, options.paneSlot)
    if (options.reveal) revealTextTab(projectId, existing.id, options.reveal)
    return existing.id
  }

  const tabId = addTextTab(projectId, filePath, options.temporary ?? true)
  placeAndFocus(projectId, tabId, options.paneSlot)
  if (options.reveal) revealTextTab(projectId, tabId, options.reveal)
  return tabId
}

export async function openTextSnapshot(
  projectId: string,
  filePath: string,
  gitRef: string,
  refLabel: string,
  options: OpenTextOptions = {}
): Promise<TabId> {
  await ensureProjectWorkspaceReady(projectId)
  const existing = getAllTabs(projectId).find((tab) => isSnapshotTextTab(tab, filePath, gitRef))
  if (existing) {
    placeAndFocus(projectId, existing.id, options.paneSlot)
    if (options.reveal) revealTextTab(projectId, existing.id, options.reveal)
    return existing.id
  }

  const tabId = addReadonlyTextTab(projectId, filePath, gitRef, refLabel, options.temporary ?? true)
  placeAndFocus(projectId, tabId, options.paneSlot)
  if (options.reveal) revealTextTab(projectId, tabId, options.reveal)
  return tabId
}

export async function createUntitledTextSurface(projectId: string, paneSlot?: PaneSlot): Promise<TabId> {
  await ensureProjectWorkspaceReady(projectId)
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

export function clearTextSurfaceDirtyIfClosed(projectId: string, tabId: TabId): void {
  const stillOpen = getAllTabs(projectId).some((tab) => tab.id === tabId)
  if (stillOpen) return
  clearTextSurfaceDirty(projectId, tabId)
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

export function clearTextSurfaceProjectState(projectId: string): void {
  const dirtyPrefix = `${projectId}::`
  for (const key of [...dirtyKeys]) {
    if (key.startsWith(dirtyPrefix)) dirtyKeys.delete(key)
  }

  for (const [tabId, ownerProjectId] of [...pendingRevealProjects]) {
    if (ownerProjectId !== projectId) continue
    pendingRevealProjects.delete(tabId)
    pendingReveals.delete(tabId)
  }

  for (const [tabId, ownerProjectId] of [...mountedControllerProjects]) {
    if (ownerProjectId !== projectId) continue
    mountedControllerProjects.delete(tabId)
    mountedControllers.delete(tabId)
  }

  discardProjectTextModelBuffers(projectId)
}

export function markTextSurfaceSaved(projectId: string, filePath: string, value: string): void {
  markTextModelBufferSaved(projectId, filePath, value)
}

export function discardTextSurfaceBuffer(projectId: string, tab: Pick<TextTab, 'id' | 'filePath' | 'gitRef'>): void {
  if (tab.gitRef) return
  if (tab.filePath == null) {
    discardUntitledTextModelBuffer(projectId, tab.id)
    return
  }
  discardTextModelBuffer(projectId, tab.filePath)
}

export async function renameTextPath(
  projectId: string,
  projectPath: string,
  oldPath: string,
  newPath: string
): Promise<void> {
  await restoreWorkspaceFromDisk(projectId)
  const tabs = getAllTabs(projectId)
  const prefix = `${oldPath}/`

  for (const tab of tabs) {
    if (tab.kind !== 'text' || tab.filePath == null) continue

    if (tab.filePath === oldPath) {
      renameTextModelBuffer(projectId, oldPath, newPath, `${projectPath}/${newPath}`)
      renameTextTab(projectId, tab.id, newPath)
      continue
    }

    if (tab.filePath.startsWith(prefix)) {
      const renamedPath = `${newPath}/${tab.filePath.slice(prefix.length)}`
      renameTextModelBuffer(projectId, tab.filePath, renamedPath, `${projectPath}/${renamedPath}`)
      renameTextTab(projectId, tab.id, renamedPath)
    }
  }
}

export async function deleteTextPath(projectId: string, path: string): Promise<void> {
  await restoreWorkspaceFromDisk(projectId)
  const tabs = getAllTabs(projectId)
  const prefix = `${path}/`

  for (const tab of tabs) {
    if (tab.kind !== 'text' || tab.filePath == null) continue
    if (tab.filePath === path || tab.filePath.startsWith(prefix)) {
      discardTextSurfaceBuffer(projectId, tab)
      closeTab(projectId, tab.id)
      clearTextSurfaceDirty(projectId, tab.id)
      pendingReveals.delete(tab.id)
      pendingRevealProjects.delete(tab.id)
      mountedControllers.delete(tab.id)
      mountedControllerProjects.delete(tab.id)
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
