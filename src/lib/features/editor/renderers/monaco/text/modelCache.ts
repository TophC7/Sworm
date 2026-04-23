type Monaco = typeof import('monaco-editor')
type MonacoModel = import('monaco-editor').editor.ITextModel
type MonacoViewState = import('monaco-editor').editor.ICodeEditorViewState

interface TextModelEntry {
  key: string
  projectId: string
  filePath: string | null
  tabId: string
  model: MonacoModel
  savedValue: string
  refs: number
  discardOnRelease: boolean
  viewState: MonacoViewState | null
  lastAccess: number
}

export interface TextModelHandle {
  model: MonacoModel
  readonly refCount: number
  restoreViewState(): MonacoViewState | null
  saveViewState(viewState: MonacoViewState | null): void
  release(): number
}

interface AcquireTextModelOptions {
  monaco: Monaco
  projectId: string | null
  tabId: string
  filePath: string | null
  uriPath: string | null
  value: string
  language: string
}

const entries = new Map<string, TextModelEntry>()
const MAX_RETAINED_TEXT_MODELS = 64
const MAX_RETAINED_TEXT_MODEL_BYTES = 50 * 1024 * 1024
let monacoRef: Monaco | null = null

function fileKey(projectId: string, filePath: string): string {
  return `${projectId}:file:${filePath}`
}

function untitledKey(projectId: string, tabId: string): string {
  return `${projectId}:untitled:${tabId}`
}

function isDisposed(model: MonacoModel): boolean {
  return model.isDisposed()
}

function disposeEntry(entry: TextModelEntry): void {
  entries.delete(entry.key)
  if (!isDisposed(entry.model)) entry.model.dispose()
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
  return !isDisposed(entry.model) && entry.model.getValue() !== entry.savedValue
}

function trimRetainedModels(): void {
  const candidates = [...entries.values()]
    .filter((entry) => entry.refs === 0 && !entry.discardOnRelease && !isDirty(entry))
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

export function acquireTextModel(options: AcquireTextModelOptions): TextModelHandle | null {
  const { monaco, projectId, tabId, filePath, uriPath, value, language } = options
  if (!projectId) return null

  monacoRef = monaco
  const key = filePath != null ? fileKey(projectId, filePath) : untitledKey(projectId, tabId)
  const existing = entries.get(key)
  if (existing && !isDisposed(existing.model)) {
    existing.discardOnRelease = false
    reconcileWithDisk(existing, value)
    touchEntry(existing)
    existing.refs += 1
    return makeHandle(existing)
  }
  if (existing) entries.delete(key)

  const uri = uriPath ? monaco.Uri.file(uriPath) : null
  const existingModel = uri ? monaco.editor.getModel(uri) : null
  if (existingModel && existingModel.getValue() !== value) {
    existingModel.setValue(value)
  }
  const model =
    existingModel ??
    (uri ? monaco.editor.createModel(value, language, uri) : monaco.editor.createModel(value, language))

  const entry: TextModelEntry = {
    key,
    projectId,
    filePath,
    tabId,
    model,
    savedValue: value,
    refs: 1,
    discardOnRelease: false,
    viewState: null,
    lastAccess: Date.now()
  }
  entries.set(key, entry)
  trimRetainedModels()
  return makeHandle(entry)
}

export function markTextModelBufferSaved(projectId: string, filePath: string, value: string): void {
  const entry = entries.get(fileKey(projectId, filePath))
  if (!entry) return
  entry.savedValue = value
  touchEntry(entry)
  trimRetainedModels()
}

export function renameTextModelBuffer(
  projectId: string,
  oldFilePath: string,
  newFilePath: string,
  newUriPath: string
): void {
  const oldKey = fileKey(projectId, oldFilePath)
  const entry = entries.get(oldKey)
  if (!entry) return

  const newKey = fileKey(projectId, newFilePath)
  const existingTarget = entries.get(newKey)
  if (existingTarget && existingTarget !== entry) {
    if (existingTarget.refs > 0) {
      existingTarget.discardOnRelease = true
      detachEntry(existingTarget)
    } else {
      disposeEntry(existingTarget)
    }
  }

  const nextModel = createRenamedModel(entry, newUriPath)
  if (!nextModel) {
    entries.delete(oldKey)
    entry.key = newKey
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
      filePath: newFilePath,
      model: nextModel,
      refs: 0,
      discardOnRelease: false,
      lastAccess: Date.now()
    })
    trimRetainedModels()
    return
  }

  entries.delete(oldKey)
  if (nextModel !== entry.model && !isDisposed(entry.model)) entry.model.dispose()
  entry.key = newKey
  entry.filePath = newFilePath
  entry.model = nextModel
  touchEntry(entry)
  entries.set(newKey, entry)
  trimRetainedModels()
}

function createRenamedModel(entry: TextModelEntry, uriPath: string): MonacoModel | null {
  if (!monacoRef) return null

  const value = entry.model.getValue()
  const language = entry.model.getLanguageId()
  const uri = monacoRef.Uri.file(uriPath)
  const existing = monacoRef.editor.getModel(uri)
  if (existing) {
    if (existing.getValue() !== value) existing.setValue(value)
    return existing
  }

  return monacoRef.editor.createModel(value, language, uri)
}

export function discardTextModelBuffer(projectId: string, filePath: string): void {
  const entry = entries.get(fileKey(projectId, filePath))
  if (!entry) return
  if (entry.refs > 0) {
    entry.discardOnRelease = true
    return
  }
  disposeEntry(entry)
}

export function discardUntitledTextModelBuffer(projectId: string, tabId: string): void {
  const entry = entries.get(untitledKey(projectId, tabId))
  if (!entry) return
  if (entry.refs > 0) {
    entry.discardOnRelease = true
    return
  }
  disposeEntry(entry)
}

export function discardProjectTextModelBuffers(projectId: string): void {
  for (const entry of [...entries.values()]) {
    if (entry.projectId !== projectId) continue
    if (entry.refs > 0) {
      entry.discardOnRelease = true
      continue
    }
    disposeEntry(entry)
  }
  trimRetainedModels()
}
