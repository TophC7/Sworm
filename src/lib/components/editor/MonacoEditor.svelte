<script lang="ts">
  import { onMount } from 'svelte'
  import { registerEditor, onEditorFocus, onEditorBlur, onEditorDestroy } from '$lib/editor/editorActions.svelte'
  import { SWORM_THEME_NAME } from '$lib/editor/monacoTheme'

  let {
    value = '',
    language = 'plaintext',
    readonly = false,
    wordWrap = false,
    onchange
  }: {
    value?: string
    language?: string
    readonly?: boolean
    wordWrap?: boolean
    onchange?: (value: string) => void
  } = $props()

  let containerEl = $state<HTMLDivElement | null>(null)
  let editor: import('monaco-editor').editor.IStandaloneCodeEditor | null = null
  let monaco: typeof import('monaco-editor') | null = null
  // Tracks the last value reported via onchange so the sync $effect
  // can distinguish editor-originated changes from external reloads.
  let lastReportedValue = ''

  onMount(() => {
    let disposed = false
    let resizeObserver: ResizeObserver | null = null

    async function init() {
      const { initMonaco } = await import('$lib/editor/monacoEnv')
      const m = await import('monaco-editor')
      if (disposed || !containerEl) return

      monaco = m
      await initMonaco(m)

      editor = m.editor.create(containerEl, {
        value,
        language,
        theme: SWORM_THEME_NAME,
        readOnly: readonly,
        minimap: { enabled: false },
        fontSize: 13,
        lineHeight: 20,
        fontFamily: "'Monocraft Nerd Font', monospace",
        fontLigatures: false,
        lineNumbers: 'on',
        renderWhitespace: 'selection',
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
        quickSuggestions: false,
        suggestOnTriggerCharacters: true,
        parameterHints: { enabled: true },
        padding: { top: 12, bottom: 12 }
      })

      lastReportedValue = value
      editor.onDidChangeModelContent(() => {
        if (editor && onchange) {
          lastReportedValue = editor.getValue()
          onchange(lastReportedValue)
        }
      })

      registerEditor(editor)
      editor.onDidFocusEditorText(() => onEditorFocus(editor!))
      editor.onDidBlurEditorText(() => onEditorBlur())

      // Observe after creation so the first layout() is correct
      resizeObserver = new ResizeObserver(() => editor?.layout())
      resizeObserver.observe(containerEl)
    }

    void init()

    return () => {
      disposed = true
      if (editor) {
        onEditorDestroy(editor)
        const model = editor.getModel()
        editor.dispose()
        model?.dispose()
      }
      resizeObserver?.disconnect()
      editor = null
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
    editor?.updateOptions({ readOnly: readonly })
  })

  $effect(() => {
    editor?.updateOptions({ wordWrap: wordWrap ? 'on' : 'off' })
  })
</script>

<div bind:this={containerEl} class="h-full w-full"></div>
