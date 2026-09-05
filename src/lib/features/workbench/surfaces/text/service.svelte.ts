import { backend } from '$lib/api/backend'
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
  generateTabId,
  getTabs,
  renameTextTab,
  setActiveTab
} from '$lib/features/workbench/state.svelte'
import { normalizeAbsolutePath, resolveProjectFile, toProjectRelativePath } from '$lib/utils/paths'

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

function isLiveTextTabUnderAbsolutePath(tab: Tab, filePath: string): tab is TextTab & { filePath: string } {
  if (tab.kind !== 'text' || tab.filePath == null || tab.gitRef) return false
  const absolutePath = resolveProjectFile(tab.folderPath, tab.filePath)
  return absolutePath === filePath || absolutePath.startsWith(`${filePath}/`)
}

function isSnapshotTextTab(tab: Tab, folderPath: string, filePath: string, gitRef: string): tab is TextTab {
  return tab.kind === 'text' && tab.folderPath === folderPath && tab.filePath === filePath && tab.gitRef === gitRef
}

let fileSyncListeners: Promise<void> | null = null

/** Install window-targeted coordinator listeners once for this webview. */
export function ensureTextFileSyncListeners(): Promise<void> {
  if (fileSyncListeners) return fileSyncListeners

  fileSyncListeners = Promise.all([
    backend.window.onFilePathChanged(({ oldPath, newPath, folderPath }) => {
      const oldRoot = normalizeAbsolutePath(oldPath)
      const newRoot = normalizeAbsolutePath(newPath)
      for (const tab of getTabs()) {
        if (!isLiveTextTabUnderAbsolutePath(tab, oldRoot)) continue

        const currentPath = resolveProjectFile(tab.folderPath, tab.filePath)
        const nextPath = `${newRoot}${currentPath.slice(oldRoot.length)}`
        let nextRelative = toProjectRelativePath(tab.folderPath, nextPath)
        let nextFolder: string | undefined
        if (nextRelative == null) {
          nextFolder = normalizeAbsolutePath(folderPath)
          nextRelative = toProjectRelativePath(nextFolder, nextPath)!
        }
        renameTextModelBuffer(tab.folderPath, tab.filePath, nextRelative, nextPath, nextFolder)
        renameTextTab(tab.id, nextRelative, nextFolder ?? tab.folderPath)
      }
    }),
    backend.window.onFileDeleted(({ filePath }) => {
      const deletedRoot = normalizeAbsolutePath(filePath)
      for (const tab of getTabs()) {
        if (!isLiveTextTabUnderAbsolutePath(tab, deletedRoot)) continue
        if (isTextSurfaceDirty(tab.id)) continue
        discardTextSurfaceBuffer(tab)
        closeTab(tab.id)
        forgetTab(tab.id)
      }
    })
  ])
    .then(() => undefined)
    .catch((error) => {
      fileSyncListeners = null
      throw error
    })
  return fileSyncListeners
}

export function revealTextTab(tabId: TabId, target: TextRevealTarget): void {
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
  if (existing) {
    setActiveTab(existing.id)
    if (options.reveal) revealTextTab(existing.id, options.reveal)
    return existing.id
  }

  await ensureTextFileSyncListeners()
  const temporary = options.temporary ?? true
  const replaced = temporary ? getTabs().find((tab): tab is TextTab => tab.kind === 'text' && tab.temporary) : undefined
  const tabId = replaced?.id ?? generateTabId()
  const absolutePath = resolveProjectFile(folderPath, filePath)
  const result = await backend.window.claimFile(absolutePath, tabId, options.reveal ?? null)
  if (result.status === 'redirect') return result.tab_id as TabId

  addTextTab(folderPath, filePath, temporary, tabId)
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
      renameTextModelBuffer(folderPath, oldPath, newPath, resolveProjectFile(folderPath, newPath))
      renameTextTab(tab.id, newPath)
      continue
    }

    if (tab.filePath.startsWith(prefix)) {
      const renamedPath = `${newPath}/${tab.filePath.slice(prefix.length)}`
      renameTextModelBuffer(folderPath, tab.filePath, renamedPath, resolveProjectFile(folderPath, renamedPath))
      renameTextTab(tab.id, renamedPath)
    }
  }
}

export function deleteTextPath(folderPath: string, path: string): void {
  const prefix = `${path}/`

  for (const tab of getTabs()) {
    if (tab.kind !== 'text' || tab.folderPath !== folderPath || tab.filePath == null) continue
    if (tab.filePath === path || tab.filePath.startsWith(prefix)) {
      if (isTextSurfaceDirty(tab.id)) continue
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
