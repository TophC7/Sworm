<script lang="ts">
  import {
    onTextEditorBlur,
    onTextEditorDestroy,
    onTextEditorFocus,
    registerTextEditorActions
  } from '$lib/features/editor/renderers/monaco/text/actions.svelte'
  import {
    attachIndentRainbow,
    isIndentRainbowEnabled
  } from '$lib/features/editor/renderers/monaco/text/indentRainbow.svelte'
  import {
    attachGitHunkReview,
    type GitHunkReviewHandle
  } from '$lib/features/editor/renderers/monaco/text/gitHunkReview'
  import {
    acquireTextModel,
    type TextModelHandle
  } from '$lib/features/editor/renderers/monaco/text/modelCache'
  import { attachLspModel, detachLspModel } from '$lib/features/editor/lsp/registry'
  import {
    registerMountedTextSurface,
    takePendingTextReveal,
    unregisterMountedTextSurface,
    type MountedTextSurfaceController
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { SWORM_THEME_NAME } from '$lib/features/editor/renderers/monaco/core/monacoTheme'
  import { MONO_FONT_FAMILY } from '$lib/fonts'
  import { isAnyModalOpen } from '$lib/utils/modalRegistry.svelte'
  import { onMount } from 'svelte'

  let {
    tabId,
    value = '',
    language = 'plaintext',
    readonly = false,
    locked = false,
    largeFile = false,
    wordWrap = false,
    onchange,
    folderPath = null,
    filePath = null,
    lspEnabled = true,
    gitDiffRevision = '',
    onready
  }: {
    tabId: string
    value?: string
    language?: string
    readonly?: boolean
    locked?: boolean
    largeFile?: boolean
    wordWrap?: boolean
    onchange?: (value: string) => void
    folderPath?: string | null
    filePath?: string | null
    lspEnabled?: boolean
    gitDiffRevision?: string
    onready?: (editor: import('monaco-editor').editor.IStandaloneCodeEditor) => void | (() => void)
  } = $props()

  let containerEl = $state<HTMLDivElement | null>(null)
  let editor = $state<import('monaco-editor').editor.IStandaloneCodeEditor | null>(null)
  let monaco = $state<typeof import('monaco-editor') | null>(null)
  let model = $state<import('monaco-editor').editor.ITextModel | null>(null)
  let modelHandle: TextModelHandle | null = null
  let indentRainbow:
    import('$lib/features/editor/renderers/monaco/text/indentRainbow.svelte').IndentRainbowHandle | null = null
  let gitHunkReview: GitHunkReviewHandle | null = null
  // Tracks the last value reported via onchange so the sync $effect
  // can distinguish editor-originated changes from external reloads.
  let lastReportedValue = ''
  let pendingAdoptedValue: string | null = null

  onMount(() => {
    let disposed = false
    let resizeObserver: ResizeObserver | null = null
    let mountedController: MountedTextSurfaceController | null = null
    let disposeReady: void | (() => void)
    async function init() {
      const { initMonaco } = await import('$lib/features/editor/renderers/monaco/core/monacoEnv')
      const m = await import('monaco-editor')
      if (disposed || !containerEl) return

      monaco = m
      await initMonaco(m)
      if (disposed || !containerEl) return

      modelHandle =
        readonly || largeFile
          ? null
          : acquireTextModel({
              monaco: m,
              folderPath,
              tabId,
              filePath,
              value,
              language
            })

      if (modelHandle) {
        model = modelHandle.model
      } else {
        model = m.editor.createModel(value, largeFile ? 'plaintext' : language)
      }

      editor = m.editor.create(containerEl, {
        model,
        theme: SWORM_THEME_NAME,
        readOnly: readonly || locked || largeFile,
        minimap: { enabled: false },
        folding: !largeFile,
        codeLens: !largeFile,
        detectIndentation: !largeFile,
        maxTokenizationLineLength: largeFile ? 0 : 20000,
        renderValidationDecorations: largeFile ? 'off' : 'editable',
        fontSize: 13,
        lineHeight: 20,
        fontFamily: MONO_FONT_FAMILY,
        fontLigatures: false,
        lineNumbers: 'on',
        renderWhitespace: 'all',
        scrollBeyondLastLine: false,
        wordWrap: wordWrap && !largeFile ? 'on' : 'off',
        tabSize: 2,
        insertSpaces: true,
        automaticLayout: false,
        smoothScrolling: true,
        cursorSmoothCaretAnimation: 'on',
        cursorBlinking: 'smooth',
        occurrencesHighlight: 'off',
        selectionHighlight: false,
        renderLineHighlight: 'line',
        matchBrackets: 'always',
        guides: { indentation: true, bracketPairs: false },
        overviewRulerLanes: 0,
        hideCursorInOverviewRuler: true,
        overviewRulerBorder: false,
        scrollbar: {
          verticalScrollbarSize: 6,
          horizontalScrollbarSize: 6,
          useShadows: false
        },
        quickSuggestions: {
          other: true,
          comments: false,
          strings: false
        },
        suggestOnTriggerCharacters: true,
        parameterHints: { enabled: true },
        padding: { top: 12, bottom: 12 }
      })
      modelHandle?.attachEditor(() => editor?.saveViewState() ?? null)

      const retainedViewState = modelHandle?.restoreViewState()
      if (retainedViewState) editor.restoreViewState(retainedViewState)

      if (lspEnabled && !largeFile && model && folderPath) {
        void attachLspModel(model, { folderPath })
      }

      mountedController = {
        focus: () => {
          if (locked) return
          editor?.focus()
        },
        reveal: (target) => {
          if (!editor) return
          if (target.kind === 'range') {
            editor.setSelection(target)
            editor.revealRangeInCenter(target)
            return
          }
          editor.setPosition(target)
          editor.revealPositionInCenter(target)
        }
      }
      registerMountedTextSurface(tabId, mountedController)

      const revealTarget = takePendingTextReveal(tabId)
      if (revealTarget) mountedController.reveal(revealTarget)

      lastReportedValue = largeFile ? value : model.getValue()
      if (lastReportedValue !== value && onchange) {
        pendingAdoptedValue = lastReportedValue
        onchange(lastReportedValue)
      }
      editor.onDidChangeModelContent(() => {
        if (editor && onchange && !largeFile) {
          lastReportedValue = editor.getValue()
          onchange(lastReportedValue)
        }
      })

      registerTextEditorActions(editor)
      editor.onDidFocusEditorText(() => onTextEditorFocus(editor!))
      editor.onDidBlurEditorText(() => onTextEditorBlur())

      if (!largeFile) indentRainbow = attachIndentRainbow(editor)
      disposeReady = onready?.(editor)

      // Observe after creation so the first layout() is correct
      resizeObserver = new ResizeObserver(() => editor?.layout())
      resizeObserver.observe(containerEl)

      // Grab keyboard focus on first mount so freshly opened file tabs
      // start in "type now" state without requiring a click. Surface
      // only mounts when its tab is active, so this runs exactly when
      // the user expects it. Modal guard keeps palette/settings focus.
      // Double-try: immediate focus covers the fast path, the rAF
      // retry covers the case where Monaco's textarea isn't attached
      // until after the next layout tick.
      if (!disposed && !locked && !isAnyModalOpen()) {
        editor.focus()
        requestAnimationFrame(() => {
          if (disposed || locked) return
          editor?.focus()
        })
      }
    }

    void init()

    return () => {
      disposed = true
      disposeReady?.()
      if (editor) {
        onTextEditorDestroy(editor)
        gitHunkReview?.dispose()
        gitHunkReview = null
        indentRainbow?.dispose()
        if (modelHandle) {
          modelHandle.detachEditor()
          modelHandle.saveViewState(editor.saveViewState())
        }
        const shouldDetachLsp =
          model != null && lspEnabled && !largeFile && (modelHandle ? modelHandle.refCount <= 1 : true)
        if (shouldDetachLsp && model) detachLspModel(model)
        if (mountedController) unregisterMountedTextSurface(tabId, mountedController)
        editor.dispose()
        if (modelHandle) {
          modelHandle.release()
          modelHandle = null
        } else {
          model?.dispose()
        }
      }
      resizeObserver?.disconnect()
      editor = null
      model = null
    }
  })

  // Sync external value changes (file reloads) into the editor.
  // Skip when the value originated from the editor itself (typing).
  $effect(() => {
    if (!editor) return
    if (pendingAdoptedValue !== null) {
      if (value === pendingAdoptedValue) pendingAdoptedValue = null
      else return
    }
    if (value === lastReportedValue) return
    if (largeFile || value !== editor.getValue()) {
      lastReportedValue = value
      editor.setValue(value)
    }
  })

  $effect(() => {
    if (!editor || !monaco) return
    const model = editor.getModel()
    if (model) monaco.editor.setModelLanguage(model, largeFile ? 'plaintext' : language)
  })

  $effect(() => {
    editor?.updateOptions({ readOnly: readonly || locked || largeFile })
  })

  $effect(() => {
    editor?.updateOptions({ wordWrap: wordWrap && !largeFile ? 'on' : 'off' })
  })

  $effect(() => {
    const revision = gitDiffRevision
    if (!editor || !monaco || !model || readonly || largeFile || !folderPath || !filePath) {
      gitHunkReview?.dispose()
      gitHunkReview = null
      return
    }

    if (!gitHunkReview) {
      gitHunkReview = attachGitHunkReview({
        monaco,
        editor,
        model,
        folderPath,
        filePath,
        language
      })
    }

    void revision
    void gitHunkReview.refreshBase()
  })

  $effect(() => {
    isIndentRainbowEnabled()
    indentRainbow?.scheduleUpdate()
  })
</script>

<div bind:this={containerEl} class="h-full w-full"></div>
