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
  import { attachLspModel, detachLspModel } from '$lib/features/editor/lsp/registry'
  import {
    registerMountedTextSurface,
    takePendingTextReveal,
    unregisterMountedTextSurface
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { SWORM_THEME_NAME } from '$lib/features/editor/renderers/monaco/core/monacoTheme'
  import { isAnyModalOpen } from '$lib/utils/modalRegistry.svelte'
  import { onMount } from 'svelte'

  let {
    tabId,
    value = '',
    language = 'plaintext',
    readonly = false,
    locked = false,
    wordWrap = false,
    onchange,
    uriPath = null,
    projectId = null,
    projectPath = null,
    lspEnabled = true
  }: {
    tabId: string
    value?: string
    language?: string
    readonly?: boolean
    locked?: boolean
    wordWrap?: boolean
    onchange?: (value: string) => void
    uriPath?: string | null
    projectId?: string | null
    projectPath?: string | null
    lspEnabled?: boolean
  } = $props()

  let containerEl = $state<HTMLDivElement | null>(null)
  let editor: import('monaco-editor').editor.IStandaloneCodeEditor | null = null
  let monaco: typeof import('monaco-editor') | null = null
  let model: import('monaco-editor').editor.ITextModel | null = null
  let indentRainbow:
    | import('$lib/features/editor/renderers/monaco/text/indentRainbow.svelte').IndentRainbowHandle
    | null = null
  // Tracks the last value reported via onchange so the sync $effect
  // can distinguish editor-originated changes from external reloads.
  let lastReportedValue = ''

  onMount(() => {
    let disposed = false
    let resizeObserver: ResizeObserver | null = null
    async function init() {
      const { initMonaco } = await import('$lib/features/editor/renderers/monaco/core/monacoEnv')
      const m = await import('monaco-editor')
      if (disposed || !containerEl) return

      monaco = m
      await initMonaco(m)

      const targetUri = uriPath ? m.Uri.file(uriPath) : null
      // LSP navigation can preload a target model before the editor tab exists.
      model = targetUri
        ? (m.editor.getModel(targetUri) ?? m.editor.createModel(value, language, targetUri))
        : m.editor.createModel(value, language)

      editor = m.editor.create(containerEl, {
        model,
        theme: SWORM_THEME_NAME,
        readOnly: readonly || locked,
        minimap: { enabled: false },
        fontSize: 13,
        lineHeight: 20,
        fontFamily: "'Monocraft Nerd Font', monospace",
        fontLigatures: false,
        lineNumbers: 'on',
        renderWhitespace: 'all',
        scrollBeyondLastLine: false,
        wordWrap: wordWrap ? 'on' : 'off',
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

      if (lspEnabled && model && projectId && projectPath) {
        void attachLspModel(model, { projectId, projectPath })
      }

      if (editor) {
        registerMountedTextSurface(tabId, {
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
        })

        const revealTarget = takePendingTextReveal(tabId)
        if (revealTarget?.kind === 'range') {
          editor.setSelection(revealTarget)
          editor.revealRangeInCenter(revealTarget)
        } else if (revealTarget?.kind === 'position') {
          editor.setPosition(revealTarget)
          editor.revealPositionInCenter(revealTarget)
        }
      }

      lastReportedValue = value
      editor.onDidChangeModelContent(() => {
        if (editor && onchange) {
          lastReportedValue = editor.getValue()
          onchange(lastReportedValue)
        }
      })

      registerTextEditorActions(editor)
      editor.onDidFocusEditorText(() => onTextEditorFocus(editor!))
      editor.onDidBlurEditorText(() => onTextEditorBlur())

      indentRainbow = attachIndentRainbow(editor)

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
      if (editor) {
        onTextEditorDestroy(editor)
        indentRainbow?.dispose()
        if (model) detachLspModel(model)
        unregisterMountedTextSurface(tabId)
        editor.dispose()
        model?.dispose()
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
    if (value === lastReportedValue) return
    if (value !== editor.getValue()) editor.setValue(value)
  })

  $effect(() => {
    if (!editor || !monaco) return
    const model = editor.getModel()
    if (model) monaco.editor.setModelLanguage(model, language)
  })

  $effect(() => {
    editor?.updateOptions({ readOnly: readonly || locked })
  })

  $effect(() => {
    editor?.updateOptions({ wordWrap: wordWrap ? 'on' : 'off' })
  })

  $effect(() => {
    isIndentRainbowEnabled()
    indentRainbow?.scheduleUpdate()
  })
</script>

<div bind:this={containerEl} class="h-full w-full"></div>
