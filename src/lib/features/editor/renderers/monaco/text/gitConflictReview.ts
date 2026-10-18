import type * as MonacoAPI from 'monaco-editor'
import { untrack } from 'svelte'
import {
  getTextEditorActions,
  registerTextEditorActions
} from '$lib/features/editor/renderers/monaco/text/actions.svelte'
import type { EditorAction } from '$lib/features/editor/renderers/monaco/text/actions.svelte'
import { notify } from '$lib/features/notifications/state.svelte'
import {
  getConflictEditRange,
  parseConflictBlocks,
  resolveConflictBlock,
  type ConflictBlock,
  type ConflictResolution
} from './conflictParser'

type Monaco = typeof MonacoAPI
type CodeEditor = MonacoAPI.editor.IStandaloneCodeEditor
type TextModel = MonacoAPI.editor.ITextModel
type Disposable = MonacoAPI.IDisposable

type Attachment = { editor: CodeEditor }
const trackedModels = new Map<TextModel, Set<Attachment>>()
let lensChanges: MonacoAPI.Emitter<MonacoAPI.languages.CodeLensProvider> | null = null
let lensProvider: MonacoAPI.languages.CodeLensProvider | null = null

const resolutions = [
  { resolution: 'current', command: 'sworm.conflict.acceptCurrent', title: 'Accept Current Change' },
  { resolution: 'incoming', command: 'sworm.conflict.acceptIncoming', title: 'Accept Incoming Change' },
  { resolution: 'both', command: 'sworm.conflict.acceptBoth', title: 'Accept Both Changes' }
] as const

function canEdit(monaco: Monaco, model: TextModel): boolean {
  const attachments = trackedModels.get(model)
  if (!attachments || model.isDisposed()) return false
  for (const { editor } of attachments) {
    if (editor.getModel() === model && !editor.getOption(monaco.editor.EditorOption.readOnly)) return true
  }
  return false
}

function applyResolutions(monaco: Monaco, model: TextModel, blocks: ConflictBlock[], resolution: ConflictResolution) {
  if (!canEdit(monaco, model) || blocks.length === 0) return
  const lines = model.getLinesContent()
  const edits = blocks.map((block) => ({
    range: model.validateRange(getConflictEditRange(block, lines.length)),
    text: resolveConflictBlock(lines, block, resolution, model.getEOL())
  }))
  model.pushStackElement()
  model.pushEditOperations([], edits, () => null)
  model.pushStackElement()
}

function ensureProvider(monaco: Monaco) {
  if (lensProvider) return
  lensChanges = new monaco.Emitter<MonacoAPI.languages.CodeLensProvider>()
  lensProvider = {
    onDidChange: lensChanges.event,
    provideCodeLenses(model) {
      if (!canEdit(monaco, model)) return { lenses: [], dispose() {} }
      const version = model.getVersionId()
      const lenses = parseConflictBlocks(model.getLinesContent()).flatMap((block) =>
        resolutions.map(({ command, title }) => ({
          range: new monaco.Range(block.startLine, 1, block.startLine, 1),
          command: {
            id: command,
            title,
            arguments: [model.uri.toString(), version, block.startLine]
          }
        }))
      )
      return { lenses, dispose() {} }
    }
  }
  monaco.languages.registerCodeLensProvider('*', lensProvider)
  for (const { command, resolution } of resolutions) {
    monaco.editor.registerCommand(command, (_accessor, uri: string, version: number, startLine: number) => {
      const model = monaco.editor.getModel(monaco.Uri.parse(uri))
      if (!model || !canEdit(monaco, model)) return
      // An index can now identify a different block after an earlier resolution.
      // Reject stale lenses rather than ever resolving the wrong live conflict.
      if (model.getVersionId() !== version) return
      const blocks = parseConflictBlocks(model.getLinesContent())
      const block = blocks.find((candidate) => candidate.startLine === startLine)
      if (block) applyResolutions(monaco, model, [block], resolution)
    })
  }
}

export function attachGitConflictReview({
  monaco,
  editor,
  model
}: {
  monaco: Monaco
  editor: CodeEditor
  model: TextModel
}): Disposable {
  ensureProvider(monaco)
  const attachment = { editor }
  const attachments = trackedModels.get(model) ?? new Set<Attachment>()
  attachments.add(attachment)
  trackedModels.set(model, attachments)

  const collection = editor.createDecorationsCollection()
  const hasConflicts = editor.createContextKey<boolean>('swormHasMergeConflicts', false)
  const disposables: Disposable[] = []
  let disposed = false
  let previousHasConflicts = false
  let registeredActions: EditorAction[] | null = null

  function refreshActions() {
    // Preserve ownership through palette blur without stealing it from another
    // editor. The registry replaces its array whenever ownership/listing changes.
    if (!editor.hasTextFocus() && untrack(getTextEditorActions) !== registeredActions) return
    registerTextEditorActions(editor)
    registeredActions = untrack(getTextEditorActions)
  }

  function update() {
    const editable = editor.getModel() === model && !editor.getOption(monaco.editor.EditorOption.readOnly)
    const blocks = editable ? parseConflictBlocks(model.getLinesContent()) : []
    const decorations: MonacoAPI.editor.IModelDeltaDecoration[] = []
    function decorate(start: number, end: number, className: string, label?: string) {
      if (start > end) return
      decorations.push({
        range: new monaco.Range(start, 1, end, model.getLineMaxColumn(end)),
        options: {
          isWholeLine: true,
          className,
          ...(label ? { after: { content: label, inlineClassName: 'sworm-conflict-label' } } : {})
        }
      })
    }
    for (const block of blocks) {
      decorate(block.startLine, block.startLine, 'sworm-conflict-header-current', ' (Current Change)')
      decorate(block.startLine + 1, (block.commonLine ?? block.splitterLine) - 1, 'sworm-conflict-body-current')
      if (block.commonLine !== null) {
        decorate(block.commonLine, block.splitterLine - 1, 'sworm-conflict-body-common')
      }
      decorate(block.splitterLine, block.splitterLine, 'sworm-conflict-splitter')
      decorate(block.splitterLine + 1, block.endLine - 1, 'sworm-conflict-body-incoming')
      decorate(block.endLine, block.endLine, 'sworm-conflict-header-incoming', ' (Incoming Change)')
    }
    collection.set(decorations)
    const nextHasConflicts = blocks.length > 0
    hasConflicts.set(nextHasConflicts)
    if (nextHasConflicts !== previousHasConflicts) {
      previousHasConflicts = nextHasConflicts
      refreshActions()
    }
    if (lensProvider) lensChanges?.fire(lensProvider)
  }

  function navigate(direction: 'next' | 'previous') {
    const blocks = parseConflictBlocks(model.getLinesContent())
    if (blocks.length === 0) return
    const line = editor.getPosition()?.lineNumber ?? 1
    if (blocks.length === 1 && line >= blocks[0].startLine && line <= blocks[0].endLine) {
      notify.info('No other merge conflicts in this file')
      return
    }
    const block =
      direction === 'next'
        ? (blocks.find((candidate) => candidate.startLine > line) ?? blocks[0])
        : (blocks.findLast((candidate) => candidate.endLine < line) ?? blocks[blocks.length - 1])
    const range = new monaco.Range(block.startLine, 1, block.startLine, 1)
    editor.setSelection(range)
    editor.revealRangeInCenter(range)
    editor.focus()
  }

  for (const direction of ['next', 'previous'] as const) {
    disposables.push(
      editor.addAction({
        id: `sworm.conflict.${direction}`,
        label: direction === 'next' ? 'Next Conflict' : 'Previous Conflict',
        precondition: 'swormHasMergeConflicts && !editorReadonly',
        run: () => navigate(direction)
      })
    )
  }
  for (const { resolution } of resolutions) {
    const title = resolution[0].toUpperCase() + resolution.slice(1)
    disposables.push(
      editor.addAction({
        id: `sworm.conflict.acceptAll${title}`,
        label: `Accept All ${title}`,
        precondition: 'swormHasMergeConflicts && !editorReadonly',
        run: () => {
          if (editor.getModel() !== model || editor.getOption(monaco.editor.EditorOption.readOnly)) return
          applyResolutions(monaco, model, parseConflictBlocks(model.getLinesContent()), resolution)
        }
      })
    )
  }
  disposables.push(
    model.onDidChangeContent(update),
    editor.onDidChangeConfiguration((event) => {
      if (event.hasChanged(monaco.editor.EditorOption.readOnly)) update()
    }),
    editor.onDidFocusEditorText(refreshActions)
  )
  update()
  refreshActions()

  return {
    dispose() {
      if (disposed) return
      disposed = true
      for (const disposable of disposables) disposable.dispose()
      collection.clear()
      hasConflicts.reset()
      attachments.delete(attachment)
      if (attachments.size === 0) trackedModels.delete(model)
      if (lensProvider) lensChanges?.fire(lensProvider)
      refreshActions()
    }
  }
}
