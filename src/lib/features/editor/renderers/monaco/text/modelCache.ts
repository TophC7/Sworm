import type { editor, Uri } from 'monaco-editor'
import type { TextModelTransferState } from '$lib/types/backend'
import type { TextTab } from '$lib/features/workbench/model'
import { resolveProjectFile, splitRemotePath } from '$lib/utils/paths'

type Monaco = typeof import('monaco-editor')
type MonacoModel = editor.ITextModel
type MonacoViewState = editor.ICodeEditorViewState
type TextTabRef = Pick<TextTab, 'id' | 'folderPath' | 'filePath' | 'gitRef'>

interface TextModelEntry {
  key: string
  folderPath: string | null
  filePath: string | null
  model: MonacoModel
  savedValue: string
  refs: number
  discardOnRelease: boolean
  viewState: MonacoViewState | null
  liveViewState: (() => MonacoViewState | null) | null
  skipNextReconcile: boolean
  lastAccess: number
}

export interface TextModelHandle {
  model: MonacoModel
  readonly refCount: number
  restoreViewState(): MonacoViewState | null
  saveViewState(viewState: MonacoViewState | null): void
  attachEditor(get: () => MonacoViewState | null): void
  detachEditor(): void
  release(): number
}

interface AcquireTextModelOptions {
  monaco: Monaco
  folderPath: string | null
  tabId: string
  filePath: string | null
  value: string
  language: string
}

const entries = new Map<string, TextModelEntry>()
const MAX_RETAINED_TEXT_MODELS = 64
const MAX_RETAINED_TEXT_MODEL_BYTES = 50 * 1024 * 1024
let monacoRef: Monaco | null = null

function fileKey(folderPath: string, filePath: string): string {
  return `${folderPath}:file:${filePath}`
}

function untitledKey(folderPath: string, tabId: string): string {
  return `${folderPath}:untitled:${tabId}`
}

/** Scheme of remote model URIs; authority identifies the server. */
export const REMOTE_MODEL_SCHEME = 'sworm'

/** A remote workspace keeps its `sworm://<server>` authority in the model URI:
 * two servers' identical absolute paths must stay distinct models.
 *
 * Split by hand rather than parsed: `Uri.parse` reads `#` and `?` in a file
 * name as a fragment or query and silently truncates the path. */
export function textModelUri(monaco: Monaco, absolutePath: string) {
  const remote = splitRemotePath(absolutePath)
  if (!remote) return monaco.Uri.file(absolutePath)
  return monaco.Uri.from({
    scheme: REMOTE_MODEL_SCHEME,
    authority: remote.server,
    path: remote.path
  })
}

function fileModelUri(monaco: Monaco, folderPath: string, filePath: string) {
  return textModelUri(monaco, resolveProjectFile(folderPath, filePath))
}

/** Workspace-path space: the shape folderPath and openTextFile speak. */
export function modelWorkspacePath(uri: Uri): string | null {
  if (uri.scheme === 'file') return uri.fsPath
  return uri.scheme === REMOTE_MODEL_SCHEME ? `${REMOTE_MODEL_SCHEME}://${uri.authority}${uri.path}` : null
}

/** Host-absolute path, without a remote model's server authority. */
export function modelHostPath(uri: Uri): string | null {
  if (uri.scheme === 'file') return uri.fsPath
  return uri.scheme === REMOTE_MODEL_SCHEME ? uri.path : null
}

function disposeEntry(entry: TextModelEntry): void {
  entries.delete(entry.key)
  if (!entry.model.isDisposed()) entry.model.dispose()
}

function detachEntry(entry: TextModelEntry): void {
  entries.delete(entry.key)
  entry.key = `${entry.key}:detached`
}

function touchEntry(entry: TextModelEntry): void {
  entry.lastAccess = Date.now()
}

function modelSizeBytes(entry: TextModelEntry): number {
  return entry.model.getValueLength() * 2
}

function isDirty(entry: TextModelEntry): boolean {
  return !entry.model.isDisposed() && entry.model.getValue() !== entry.savedValue
}

function trimRetainedModels(): void {
  const candidates = [...entries.values()]
    .filter((entry) => entry.refs === 0 && !entry.discardOnRelease && !entry.skipNextReconcile && !isDirty(entry))
    .sort((a, b) => a.lastAccess - b.lastAccess)

  let retainedBytes = [...entries.values()].reduce((sum, entry) => sum + modelSizeBytes(entry), 0)
  for (const entry of candidates) {
    if (entries.size <= MAX_RETAINED_TEXT_MODELS && retainedBytes <= MAX_RETAINED_TEXT_MODEL_BYTES) return
    retainedBytes -= modelSizeBytes(entry)
    disposeEntry(entry)
  }
}

function makeHandle(entry: TextModelEntry): TextModelHandle {
  let released = false
  return {
    model: entry.model,
    get refCount() {
      return entry.refs
    },
    restoreViewState() {
      return entry.viewState
    },
    saveViewState(viewState) {
      entry.viewState = viewState
    },
    attachEditor(get) {
      entry.liveViewState = get
    },
    detachEditor() {
      entry.liveViewState = null
    },
    release() {
      if (released) return entry.refs
      released = true
      entry.refs = Math.max(0, entry.refs - 1)
      touchEntry(entry)
      if (entry.refs === 0 && entry.discardOnRelease) {
        disposeEntry(entry)
      }
      trimRetainedModels()
      return entry.refs
    }
  }
}

function reconcileWithDisk(entry: TextModelEntry, diskValue: string): void {
  if (entry.savedValue === diskValue) return

  const currentValue = entry.model.getValue()
  if (currentValue === entry.savedValue) {
    entry.model.setValue(diskValue)
    entry.viewState = null
  }

  entry.savedValue = diskValue
}

/** Reopen dirty buffers without reading a potentially much larger replacement. */
export function retainedTextModelBase(folderPath: string, filePath: string): string | null {
  const entry = entries.get(fileKey(folderPath, filePath))
  return entry && !entry.model.isDisposed() ? entry.savedValue : null
}

export function acquireTextModel(options: AcquireTextModelOptions): TextModelHandle | null {
  const { monaco, folderPath, tabId, filePath, value, language } = options
  if (!folderPath) return null

  monacoRef = monaco
  const key = filePath != null ? fileKey(folderPath, filePath) : untitledKey(folderPath, tabId)
  const existing = entries.get(key)
  if (existing && !existing.model.isDisposed()) {
    existing.discardOnRelease = false
    if (existing.skipNextReconcile) {
      existing.skipNextReconcile = false
    } else {
      reconcileWithDisk(existing, value)
    }
    touchEntry(existing)
    existing.refs += 1
    return makeHandle(existing)
  }
  if (existing) entries.delete(key)

  const uri = filePath != null ? fileModelUri(monaco, folderPath, filePath) : null
  const existingModel = uri ? monaco.editor.getModel(uri) : null
  if (existingModel && existingModel.getValue() !== value) {
    existingModel.setValue(value)
  }
  const model =
    existingModel ??
    (uri ? monaco.editor.createModel(value, language, uri) : monaco.editor.createModel(value, language))

  const entry: TextModelEntry = {
    key,
    filePath,
    folderPath,
    model,
    savedValue: value,
    refs: 1,
    discardOnRelease: false,
    viewState: null,
    liveViewState: null,
    skipNextReconcile: false,
    lastAccess: Date.now()
  }
  entries.set(key, entry)
  trimRetainedModels()
  return makeHandle(entry)
}

export function markTextModelBufferSaved(folderPath: string, filePath: string, value: string): void {
  const entry = entries.get(fileKey(folderPath, filePath))
  if (!entry) return
  entry.savedValue = value
  touchEntry(entry)
  trimRetainedModels()
}

export function renameTextModelBuffer(
  folderPath: string,
  oldFilePath: string,
  newFilePath: string,
  newFolderPath?: string | null
): void {
  const oldKey = fileKey(folderPath, oldFilePath)
  const entry = entries.get(oldKey)
  if (!entry) return

  const targetFolder = newFolderPath ?? folderPath
  const newKey = fileKey(targetFolder, newFilePath)
  const existingTarget = entries.get(newKey)
  if (existingTarget && existingTarget !== entry) {
    if (existingTarget.refs > 0) {
      existingTarget.discardOnRelease = true
      detachEntry(existingTarget)
    } else {
      disposeEntry(existingTarget)
    }
  }

  const nextModel = createRenamedModel(entry, targetFolder, newFilePath)
  if (!nextModel) {
    entries.delete(oldKey)
    entry.key = newKey
    entry.folderPath = targetFolder
    entry.filePath = newFilePath
    touchEntry(entry)
    entries.set(newKey, entry)
    return
  }

  if (entry.refs > 0) {
    entry.discardOnRelease = true
    entries.set(newKey, {
      ...entry,
      key: newKey,
      folderPath: targetFolder,
      filePath: newFilePath,
      model: nextModel,
      refs: 0,
      discardOnRelease: false,
      viewState: entry.liveViewState?.() ?? entry.viewState,
      liveViewState: null,
      lastAccess: Date.now()
    })
    trimRetainedModels()
    return
  }

  entries.delete(oldKey)
  if (nextModel !== entry.model && !entry.model.isDisposed()) entry.model.dispose()
  entry.key = newKey
  entry.folderPath = targetFolder
  entry.filePath = newFilePath
  entry.model = nextModel
  touchEntry(entry)
  entries.set(newKey, entry)
  trimRetainedModels()
}

function createRenamedModel(entry: TextModelEntry, folderPath: string, filePath: string): MonacoModel | null {
  if (!monacoRef) return null

  const value = entry.model.getValue()
  const language = entry.model.getLanguageId()
  const uri = fileModelUri(monacoRef, folderPath, filePath)
  const existing = monacoRef.editor.getModel(uri)
  if (existing) {
    if (existing.getValue() !== value) existing.setValue(value)
    return existing
  }

  return monacoRef.editor.createModel(value, language, uri)
}

export function discardTextModelBuffer(folderPath: string, filePath: string): void {
  const entry = entries.get(fileKey(folderPath, filePath))
  if (!entry) return
  if (entry.refs > 0) {
    entry.discardOnRelease = true
    return
  }
  disposeEntry(entry)
}

export function discardUntitledTextModelBuffer(folderPath: string, tabId: string): void {
  const entry = entries.get(untitledKey(folderPath, tabId))
  if (!entry) return
  if (entry.refs > 0) {
    entry.discardOnRelease = true
    return
  }
  disposeEntry(entry)
}

function transferEntry(tab: TextTabRef): TextModelEntry | undefined {
  if (tab.gitRef) return undefined
  return entries.get(
    tab.filePath != null ? fileKey(tab.folderPath, tab.filePath) : untitledKey(tab.folderPath, tab.id)
  )
}

export function exportModelTransfer(tab: TextTabRef): TextModelTransferState | null {
  const entry = transferEntry(tab)
  if (!entry || entry.model.isDisposed()) return null
  return {
    tabId: tab.id,
    folderPath: entry.folderPath,
    filePath: entry.filePath,
    value: entry.model.getValue(),
    savedValue: entry.savedValue,
    language: entry.model.getLanguageId(),
    viewState: entry.liveViewState?.() ?? entry.viewState
  }
}

export function importModelTransfer(state: TextModelTransferState, monaco: Monaco): void {
  monacoRef = monaco
  const key =
    state.filePath != null && state.folderPath != null
      ? fileKey(state.folderPath, state.filePath)
      : untitledKey(state.folderPath ?? '', state.tabId)
  const cached = entries.get(key)
  if (cached?.refs) throw new Error(`Text model ${state.tabId} is already mounted`)
  if (cached) disposeEntry(cached)

  const uri =
    state.filePath != null && state.folderPath != null
      ? fileModelUri(monaco, state.folderPath, state.filePath)
      : null
  const model = uri ? monaco.editor.getModel(uri) : null
  // setValue clears Monaco's local undo/redo stack; cross-realm history is unsupported.
  const transferredModel = model ?? monaco.editor.createModel(state.value, state.language, uri ?? undefined)
  if (model) {
    transferredModel.setValue(state.value)
  }
  monaco.editor.setModelLanguage(transferredModel, state.language)

  entries.set(key, {
    key,
    folderPath: state.folderPath,
    filePath: state.filePath,
    model: transferredModel,
    savedValue: state.savedValue,
    refs: 0,
    discardOnRelease: false,
    viewState: state.viewState as MonacoViewState | null,
    liveViewState: null,
    skipNextReconcile: true,
    lastAccess: Date.now()
  })
  trimRetainedModels()
}

export function detachForTransfer(tab: TextTabRef): void {
  const entry = transferEntry(tab)
  if (!entry) return
  entry.discardOnRelease = true
  if (entry.refs > 0) {
    detachEntry(entry)
  } else {
    disposeEntry(entry)
  }
}
