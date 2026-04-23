<!--
  @component
  JsonSettingsEditorView — inline Monaco JSON editor for language
  server preferences inside the Settings shell.
-->

<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { SWORM_THEME_NAME } from '$lib/features/editor/renderers/monaco/core/monacoTheme'
  import { registerSchema, unregisterSchema } from '$lib/features/editor/schemas/registry'
  import { CircleAlert } from '$lib/icons/lucideExports'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

  let {
    editorId,
    schema,
    defaults,
    value,
    description,
    onSave
  }: {
    editorId: string
    schema: unknown
    defaults: unknown
    value: string
    description: string
    onSave: (nextValue: string | null) => void | Promise<void>
  } = $props()

  let editorEl = $state<HTMLDivElement | null>(null)
  let editor: import('monaco-editor').editor.IStandaloneCodeEditor | null = null
  let monaco: typeof import('monaco-editor') | null = null
  let model: import('monaco-editor').editor.ITextModel | null = null
  let currentValue = $state(seedValue())
  let validationError = $state<string | null>(null)
  let ready = $state(false)
  let saving = $state(false)

  const editorSlug = $derived(
    editorId
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'settings'
  )
  const modelUri = $derived(`file:///__sworm_settings__/${editorSlug}.json`)
  const registryId = $derived(`settings:${editorSlug}`)

  function seedValue(): string {
    if (value.trim()) return value
    if (defaults != null) return JSON.stringify(defaults, null, 2)
    return '{}'
  }

  $effect(() => {
    if (!editorEl) return
    let disposed = false
    ready = false
    ;(async () => {
      const { initMonaco } = await import('$lib/features/editor/renderers/monaco/core/monacoEnv')
      const m = await import('monaco-editor')
      if (disposed || !editorEl) return

      monaco = m
      await initMonaco(m)

      const seeded = seedValue()
      currentValue = seeded
      validationError = null

      const resource = m.Uri.parse(modelUri)
      if (schema) {
        registerSchema({
          id: registryId,
          fileMatch: [resource.toString()],
          schema
        })
      }

      m.editor.getModel(resource)?.dispose()
      model = m.editor.createModel(seeded, 'json', resource)

      editor = m.editor.create(editorEl, {
        model,
        theme: SWORM_THEME_NAME,
        minimap: { enabled: false },
        fontSize: 13,
        lineHeight: 20,
        fontFamily: "'Monocraft Nerd Font', monospace",
        fontLigatures: false,
        lineNumbers: 'on',
        renderWhitespace: 'selection',
        scrollBeyondLastLine: false,
        wordWrap: 'on',
        tabSize: 2,
        insertSpaces: true,
        automaticLayout: true,
        formatOnPaste: false,
        formatOnType: false,
        padding: { top: 12, bottom: 12 },
        scrollbar: {
          verticalScrollbarSize: 8,
          horizontalScrollbarSize: 8,
          useShadows: false
        },
        quickSuggestions: { other: true, strings: true, comments: false },
        suggestOnTriggerCharacters: true
      })

      editor.onDidChangeModelContent(() => {
        currentValue = editor!.getValue()
        if (validationError) validationError = null
      })

      editor.addCommand(m.KeyMod.CtrlCmd | m.KeyCode.KeyS, () => {
        void handleSave()
      })

      ready = true
      editor.focus()
    })().catch((error) => {
      validationError = `Failed to open editor: ${String(error)}`
      ready = false
    })

    return () => {
      disposed = true
      ready = false
      unregisterSchema(registryId)
      editor?.dispose()
      editor = null
      model?.dispose()
      model = null
    }
  })

  async function errorMarkers(): Promise<import('monaco-editor').editor.IMarker[]> {
    if (!monaco || !model) return []
    await new Promise((resolve) => setTimeout(resolve, 0))
    return monaco.editor
      .getModelMarkers({ resource: model.uri })
      .filter((marker) => marker.severity === monaco!.MarkerSeverity.Error)
  }

  async function handleSave() {
    if (saving || !ready) return

    const trimmed = currentValue.trim()
    if (!trimmed) {
      await persistValue(null)
      return
    }

    let parsed: unknown
    try {
      parsed = JSON.parse(trimmed)
    } catch (error) {
      validationError = `Invalid JSON: ${error instanceof Error ? error.message : String(error)}`
      return
    }

    const markers = await errorMarkers()
    if (markers.length > 0) {
      validationError = markers[0]?.message ?? 'Fix JSON validation errors before saving.'
      return
    }

    await persistValue(JSON.stringify(parsed, null, 2))
  }

  async function persistValue(nextValue: string | null) {
    saving = true
    validationError = null
    try {
      await onSave(nextValue)
    } catch (error) {
      validationError = `Save failed: ${getErrorMessage(error)}`
    } finally {
      saving = false
    }
  }
</script>

<div class="flex h-full min-h-0 flex-col bg-ground">
  <div class="relative min-h-0 flex-1 bg-ground">
    <div bind:this={editorEl} class="absolute inset-0"></div>
  </div>

  <div class="flex items-center justify-between gap-4 border-t border-edge bg-surface px-5 py-3">
    <div class="min-w-0 flex-1">
      {#if validationError}
        <div class="flex items-center gap-2 text-xs text-danger-bright">
          <CircleAlert size={14} />
          <span class="truncate font-mono">{validationError}</span>
        </div>
      {:else}
        <p class="truncate text-xs text-subtle">
          {description} Use <span class="font-mono">Ctrl+Space</span> for suggestions.
        </p>
      {/if}
    </div>

    <Button size="sm" variant="accent" onclick={handleSave} disabled={saving || !ready}>
      {saving ? 'Saving…' : 'Save'}
    </Button>
  </div>
</div>
