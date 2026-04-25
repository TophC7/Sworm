import { mount, unmount } from 'svelte'
import { backend } from '$lib/api/backend'
import DirtyDiffPeekHeader from '$lib/features/editor/renderers/monaco/text/DirtyDiffPeekHeader.svelte'
import { refreshGit } from '$lib/features/git/state.svelte'
import {
  applyChangeHunks,
  compareHunks,
  computeChangeHunks,
  hunkRevealLine,
  hunksIntersectOrTouch,
  lineIntersectsHunk,
  type ChangeHunk
} from '$lib/features/git/hunks'
import { basename } from '$lib/utils/paths'

const SWORM_DIRTY_DIFF_GLYPH_CLASS = 'sworm-dirty-diff-glyph'

type Monaco = typeof import('monaco-editor')
type CodeEditor = import('monaco-editor').editor.IStandaloneCodeEditor
type DiffEditor = import('monaco-editor').editor.IStandaloneDiffEditor
type TextModel = import('monaco-editor').editor.ITextModel
type DecorationsCollection = import('monaco-editor').editor.IEditorDecorationsCollection
type OverlayWidget = import('monaco-editor').editor.IOverlayWidget
type IDisposable = import('monaco-editor').IDisposable
type MountedHeader = ReturnType<typeof mount>

type QuickDiffProvider = 'primary' | 'secondary'

interface GitHunkReviewOptions {
  monaco: Monaco
  editor: CodeEditor
  model: TextModel
  projectId: string
  projectPath: string
  filePath: string
  language: string
}

interface ProviderHunk extends ChangeHunk {
  provider: QuickDiffProvider
}

interface ActivePeek {
  index: number
  zoneId: string
  overlayWidget: OverlayWidget
  root: HTMLElement
  body: HTMLElement
  diffEditor: DiffEditor
  originalModel: TextModel
  header: MountedHeader | null
  disposables: IDisposable[]
  resizeObserver: ResizeObserver
}

export interface GitHunkReviewHandle {
  refreshBase(): Promise<void>
  scheduleUpdate(): void
  dispose(): void
}

function providerLabel(provider: QuickDiffProvider): string {
  return provider === 'primary' ? 'Git Local Changes (Working Tree)' : 'Git Local Changes (Index)'
}

function hunkClassName(hunk: ProviderHunk): string {
  return `${SWORM_DIRTY_DIFF_GLYPH_CLASS} sworm-dirty-diff-${hunk.kind} ${hunk.provider}`
}

function providerHunk(hunk: ChangeHunk, provider: QuickDiffProvider): ProviderHunk {
  return { ...hunk, provider, id: `${provider}:${hunk.id}` }
}

function primaryBase(indexContent: string | null, headContent: string | null): string | null {
  return indexContent ?? headContent
}

function hunkLineSpan(hunk: ChangeHunk): number {
  const modified = hunk.modifiedEndLineNumber > 0 ? hunk.modifiedEndLineNumber - hunk.modifiedStartLineNumber + 1 : 1
  const original = hunk.originalEndLineNumber > 0 ? hunk.originalEndLineNumber - hunk.originalStartLineNumber + 1 : 1
  return Math.max(1, modified, original)
}

function providerIndexFor(hunks: readonly ProviderHunk[], target: ProviderHunk): number {
  return hunks.filter((hunk) => hunk.provider === target.provider).findIndex((hunk) => hunk.id === target.id)
}

function providerCount(hunks: readonly ProviderHunk[], provider: QuickDiffProvider): number {
  return hunks.filter((hunk) => hunk.provider === provider).length
}

function matchingStagedHunk(stagedHunks: readonly ChangeHunk[], secondary: ChangeHunk): ChangeHunk | null {
  return (
    stagedHunks.find(
      (hunk) =>
        hunk.originalStartLineNumber === secondary.originalStartLineNumber &&
        hunk.originalEndLineNumber === secondary.originalEndLineNumber
    ) ??
    stagedHunks.find((hunk) => hunksIntersectOrTouch(hunk, secondary)) ??
    null
  )
}

function modelFullRange(model: TextModel, monaco: Monaco) {
  const lastLine = model.getLineCount()
  return new monaco.Range(1, 1, lastLine, model.getLineMaxColumn(lastLine))
}

function peekWidth(layoutInfo: import('monaco-editor').editor.EditorLayoutInfo): number {
  return layoutInfo.width - layoutInfo.minimap.minimapWidth - layoutInfo.verticalScrollbarWidth
}

function peekLeft(layoutInfo: import('monaco-editor').editor.EditorLayoutInfo): number {
  return layoutInfo.minimap.minimapWidth > 0 && layoutInfo.minimap.minimapLeft === 0
    ? layoutInfo.minimap.minimapWidth
    : 0
}

function isolateNestedEditorEvents(root: HTMLElement): IDisposable {
  const events = [
    'mousedown',
    'mouseup',
    'click',
    'dblclick',
    'contextmenu',
    'wheel',
    'pointerdown',
    'pointerup',
    'touchstart',
    'touchend',
    'keydown',
    'keyup'
  ]
  const stop = (event: Event) => event.stopPropagation()

  for (const event of events) root.addEventListener(event, stop)

  return {
    dispose() {
      for (const event of events) root.removeEventListener(event, stop)
    }
  }
}

function closeOnEscape(root: HTMLElement, onClose: () => void): IDisposable {
  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    onClose()
  }

  root.addEventListener('keydown', onKeyDown, { capture: true })

  return {
    dispose() {
      root.removeEventListener('keydown', onKeyDown, { capture: true })
    }
  }
}

export function attachGitHunkReview(options: GitHunkReviewOptions): GitHunkReviewHandle {
  const { monaco, editor, model, projectId, projectPath, filePath, language } = options
  const collection: DecorationsCollection = editor.createDecorationsCollection()
  const disposables: IDisposable[] = []

  let disposed = false
  let indexContent: string | null = null
  let headContent: string | null = null
  let hasIndexChanges = false
  let baseLoaded = false
  let primaryHunks: ChangeHunk[] = []
  let secondaryHunks: ChangeHunk[] = []
  let stagedIndexHunks: ChangeHunk[] = []
  let visibleHunks: ProviderHunk[] = []
  let updateTimer: number | null = null
  let updateSeq = 0
  let activePeek: ActivePeek | null = null

  function clearPeek(refocus = false) {
    const peek = activePeek
    if (!peek) return
    activePeek = null

    for (const disposable of peek.disposables) disposable.dispose()
    peek.resizeObserver.disconnect()
    peek.diffEditor.dispose()
    peek.originalModel.dispose()
    if (peek.header) void unmount(peek.header)

    editor.removeOverlayWidget(peek.overlayWidget)
    editor.changeViewZones((accessor) => accessor.removeZone(peek.zoneId))
    if (refocus) editor.focus()
  }

  function renderDecorations() {
    collection.set(
      visibleHunks.map((hunk) => {
        const line = hunkRevealLine(hunk)
        const endLine = hunk.kind === 'delete' ? line : Math.max(line, hunk.modifiedEndLineNumber)
        return {
          range: new monaco.Range(
            line,
            hunk.kind === 'delete' ? Number.MAX_SAFE_INTEGER : 1,
            endLine,
            hunk.kind === 'delete' ? Number.MAX_SAFE_INTEGER : 1
          ),
          options: {
            description: `sworm-dirty-diff-${hunk.provider}`,
            isWholeLine: hunk.kind !== 'delete',
            linesDecorationsClassName: hunkClassName(hunk),
            linesDecorationsTooltip: providerLabel(hunk.provider)
          }
        }
      })
    )
  }

  function rebuildVisibleHunks() {
    const primary = primaryHunks.map((hunk) => providerHunk(hunk, 'primary'))
    const secondary = secondaryHunks
      .filter((hunk) => !primaryHunks.some((primaryHunk) => hunksIntersectOrTouch(primaryHunk, hunk)))
      .map((hunk) => providerHunk(hunk, 'secondary'))

    visibleHunks = [...primary, ...secondary].sort((a, b) => compareHunks(a, b) || (a.provider === 'primary' ? -1 : 1))
  }

  function revealPeekHunk(peek: ActivePeek, hunk: ProviderHunk) {
    const modified = peek.diffEditor.getModifiedEditor()
    let start: number
    let end: number

    if (hunk.modifiedEndLineNumber === 0) {
      start = hunk.modifiedStartLineNumber
      end = hunk.modifiedStartLineNumber + 1
    } else if (hunk.originalEndLineNumber > 0) {
      start = hunk.modifiedStartLineNumber - 1
      end = hunk.modifiedEndLineNumber + 1
    } else {
      start = hunk.modifiedStartLineNumber
      end = hunk.modifiedEndLineNumber
    }

    const maxLine = modified.getModel()?.getLineCount() ?? 1
    start = Math.min(maxLine, Math.max(1, start))
    end = Math.min(maxLine, Math.max(start, end))
    modified.revealLinesInCenter(start, end)
  }

  function layoutPeek(peek: ActivePeek) {
    const headerHeight = 32
    const height = Math.max(80, peek.root.clientHeight - headerHeight)
    peek.body.style.height = `${height}px`
    peek.diffEditor.layout({ width: peek.body.clientWidth, height })
  }

  function showHunkAtIndex(index: number, refocus = true) {
    const hunk = visibleHunks[index]
    if (!hunk) return

    clearPeek(false)

    const providerSpecificIndex = providerIndexFor(visibleHunks, hunk)
    const providerSpecificCount = providerCount(visibleHunks, hunk.provider)
    const detail = `${providerLabel(hunk.provider)} - ${providerSpecificIndex + 1} of ${providerSpecificCount} ${
      providerSpecificCount === 1 ? 'change' : 'changes'
    }`

    const lineHeight = editor.getOption(monaco.editor.EditorOption.lineHeight)
    const editorHeight = editor.getLayoutInfo().height
    const editorHeightInLines = Math.max(8, Math.floor(editorHeight / lineHeight))
    const heightInLines = Math.min(
      Math.max(hunkLineSpan(hunk) + 8, 9),
      Math.max(9, Math.floor(editorHeightInLines / 2))
    )
    const heightInPx = Math.round(heightInLines * lineHeight + 32)

    const root = document.createElement('div')
    root.className = `sworm-dirty-diff-peek ${hunk.provider}`
    root.style.top = '-1000px'

    const header = document.createElement('div')
    const body = document.createElement('div')
    body.className = 'sworm-dirty-diff-peek-body'
    root.append(header, body)

    // Match Monaco's ZoneWidget shape: the view zone reserves space, while the overlay hosts interactive content.
    const zoneSpacer = document.createElement('div')
    zoneSpacer.style.overflow = 'hidden'
    let zoneHeight = heightInPx
    const overlayWidget: OverlayWidget = {
      getId: () => `sworm.dirtyDiffPeek.${projectId}.${filePath}`,
      getDomNode: () => root,
      getPosition: () => null
    }
    const layoutFrame = () => {
      const layoutInfo = editor.getLayoutInfo()
      root.style.left = `${peekLeft(layoutInfo)}px`
      root.style.width = `${peekWidth(layoutInfo)}px`
      root.style.height = `${zoneHeight}px`
      if (activePeek?.root === root) layoutPeek(activePeek)
    }

    let zoneId = ''
    editor.changeViewZones((accessor) => {
      zoneId = accessor.addZone({
        afterLineNumber: hunkRevealLine(hunk),
        domNode: zoneSpacer,
        heightInPx,
        suppressMouseDown: false,
        onDomNodeTop: (top) => {
          root.style.top = `${top}px`
        },
        onComputedHeight: (height) => {
          zoneHeight = height
          layoutFrame()
        }
      })
    })
    editor.addOverlayWidget(overlayWidget)
    layoutFrame()

    const originalContent =
      hunk.provider === 'primary' ? (primaryBase(indexContent, headContent) ?? '') : (headContent ?? '')
    const originalModel = monaco.editor.createModel(originalContent, language)
    const diffEditor = monaco.editor.createDiffEditor(body, {
      diffAlgorithm: 'advanced',
      fixedOverflowWidgets: true,
      folding: false,
      glyphMargin: false,
      ignoreTrimWhitespace: false,
      lineDecorationsWidth: 12,
      lineNumbersMinChars: 4,
      minimap: { enabled: false },
      originalEditable: false,
      readOnly: false,
      renderGutterMenu: false,
      renderIndicators: false,
      renderMarginRevertIcon: false,
      renderOverviewRuler: false,
      renderSideBySide: false,
      scrollBeyondLastLine: false,
      stickyScroll: { enabled: false },
      automaticLayout: false
    })
    diffEditor.setModel({ original: originalModel, modified: model })

    const peek: ActivePeek = {
      index,
      zoneId,
      overlayWidget,
      root,
      body,
      diffEditor,
      originalModel,
      header: null,
      disposables: [],
      resizeObserver: new ResizeObserver(() => requestAnimationFrame(() => layoutPeek(peek)))
    }
    activePeek = peek

    peek.header = mount(DirtyDiffPeekHeader, {
      target: header,
      props: {
        title: basename(filePath),
        detail,
        stageLabel: hunk.provider === 'primary' ? 'Stage hunk' : 'Unstage hunk',
        stageKind: hunk.provider === 'primary' ? 'stage' : 'unstage',
        revertLabel: hunk.provider === 'primary' ? 'Revert hunk' : '',
        canStage: true,
        canRevert: hunk.provider === 'primary',
        onStage: () => (hunk.provider === 'primary' ? stageHunk(hunk) : unstageHunk(hunk)),
        onRevert: () => revertHunk(hunk),
        onPrevious: () => showHunkAtIndex((index - 1 + visibleHunks.length) % visibleHunks.length, false),
        onNext: () => showHunkAtIndex((index + 1) % visibleHunks.length, false),
        onClose: () => clearPeek(true)
      }
    })

    peek.resizeObserver.observe(root)
    peek.disposables.push(
      closeOnEscape(root, () => clearPeek(true)),
      isolateNestedEditorEvents(root),
      editor.onDidLayoutChange(() => layoutFrame()),
      diffEditor.onDidUpdateDiff(() => requestAnimationFrame(() => revealPeekHunk(peek, hunk)))
    )

    requestAnimationFrame(() => {
      layoutPeek(peek)
      revealPeekHunk(peek, hunk)
      if (refocus) diffEditor.getModifiedEditor().focus()
    })

    editor.revealLineInCenter(hunkRevealLine(hunk))
  }

  function showHunk(hunk: ProviderHunk) {
    const index = visibleHunks.findIndex((candidate) => candidate.id === hunk.id)
    if (index < 0) return
    if (activePeek?.index === index) {
      clearPeek(true)
      return
    }
    showHunkAtIndex(index)
  }

  async function stageHunk(hunk: ProviderHunk) {
    if (hunk.provider !== 'primary') return
    const base = primaryBase(indexContent, headContent)
    if (base == null) return

    const nextIndex = applyChangeHunks(base, model.getValue(), [hunk])
    await backend.git.stageFileContent(projectPath, filePath, nextIndex)
    clearPeek(false)
    await refreshGit(projectId, projectPath)
    await refreshBase()
  }

  function replaceEditorContent(content: string) {
    if (content === model.getValue()) return
    editor.pushUndoStop()
    editor.executeEdits('sworm-dirty-diff-revert', [
      {
        range: modelFullRange(model, monaco),
        text: content,
        forceMoveMarkers: true
      }
    ])
    editor.pushUndoStop()
  }

  function revertHunk(hunk: ProviderHunk) {
    if (hunk.provider !== 'primary') return
    const base = primaryBase(indexContent, headContent)
    if (base == null) return

    const remaining = primaryHunks.filter((candidate) => `primary:${candidate.id}` !== hunk.id)
    const nextContent = applyChangeHunks(base, model.getValue(), remaining)
    clearPeek(false)
    replaceEditorContent(nextContent)
    scheduleUpdate()
  }

  async function unstageHunk(hunk: ProviderHunk) {
    if (hunk.provider !== 'secondary') return
    const base = headContent ?? ''
    const currentIndex = indexContent ?? ''
    const stagedHunk = matchingStagedHunk(stagedIndexHunks, hunk)
    if (!stagedHunk) return

    const remaining = stagedIndexHunks.filter((candidate) => candidate.id !== stagedHunk.id)
    const nextIndex =
      headContent == null && remaining.length === 0 ? null : applyChangeHunks(base, currentIndex, remaining)

    await backend.git.stageFileContent(projectPath, filePath, nextIndex)
    clearPeek(false)
    await refreshGit(projectId, projectPath)
    await refreshBase()
  }

  async function update() {
    if (disposed || !baseLoaded) return
    const seq = ++updateSeq
    const current = model.getValue()
    const base = primaryBase(indexContent, headContent)

    const [nextPrimary, nextSecondary] = await Promise.all([
      base == null ? Promise.resolve([]) : computeChangeHunks(monaco, base, current, language),
      hasIndexChanges ? computeChangeHunks(monaco, headContent ?? '', current, language) : Promise.resolve([])
    ])

    if (disposed || seq !== updateSeq) return
    primaryHunks = nextPrimary
    secondaryHunks = nextSecondary
    rebuildVisibleHunks()
    renderDecorations()

    if (activePeek) {
      if (activePeek.index >= visibleHunks.length) clearPeek(false)
      else showHunkAtIndex(activePeek.index, false)
    }
  }

  function scheduleUpdate() {
    if (updateTimer) window.clearTimeout(updateTimer)
    updateTimer = window.setTimeout(() => {
      updateTimer = null
      void update()
    }, 250)
  }

  async function refreshBase() {
    try {
      const data = await backend.git.getQuickDiffData(projectPath, filePath)
      if (disposed) return
      indexContent = data.indexContent
      headContent = data.headContent
      hasIndexChanges = data.hasIndexChanges
      // Recompute only when HEAD/index changed; the result is independent of editor content.
      stagedIndexHunks =
        hasIndexChanges && indexContent != null
          ? await computeChangeHunks(monaco, headContent ?? '', indexContent, language)
          : []
      if (disposed) return
      baseLoaded = true
      scheduleUpdate()
    } catch (error) {
      console.warn('git-quick-diff-data:', error)
      baseLoaded = false
      indexContent = null
      headContent = null
      hasIndexChanges = false
      primaryHunks = []
      secondaryHunks = []
      stagedIndexHunks = []
      visibleHunks = []
      clearPeek(false)
      collection.clear()
    }
  }

  disposables.push(
    model.onDidChangeContent(scheduleUpdate),
    editor.onKeyDown((event) => {
      if (event.keyCode !== monaco.KeyCode.Escape || !activePeek) return
      event.preventDefault()
      event.stopPropagation()
      clearPeek(true)
    }),
    editor.onMouseDown((event) => {
      const range = event.target.range
      const element = event.target.element
      if (!range || !element) return
      if (event.target.type !== monaco.editor.MouseTargetType.GUTTER_LINE_DECORATIONS) return
      if (!String(element.className).includes(SWORM_DIRTY_DIFF_GLYPH_CLASS)) return

      const hunk =
        visibleHunks.find(
          (candidate) => candidate.provider === 'primary' && lineIntersectsHunk(range.startLineNumber, candidate)
        ) ?? visibleHunks.find((candidate) => lineIntersectsHunk(range.startLineNumber, candidate))
      if (!hunk) return

      event.event.preventDefault()
      event.event.stopPropagation()
      showHunk(hunk)
    })
  )

  return {
    refreshBase,
    scheduleUpdate,
    dispose() {
      disposed = true
      if (updateTimer) window.clearTimeout(updateTimer)
      clearPeek(false)
      collection.clear()
      for (const disposable of disposables) disposable.dispose()
    }
  }
}
