<script lang="ts">
  import { untrack } from 'svelte'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { IconButton } from '$lib/components/ui/button'
  import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import ContentToolbar from '$lib/components/ContentToolbar.svelte'
  import MonacoEditor from '$lib/components/editor/MonacoEditor.svelte'
  import { filePathToLanguage, isBinaryFile, isMarkdownFile } from '$lib/editor/languageMap'
  import MarkdownRenderer from '$lib/components/markdown/MarkdownRenderer.svelte'
  import { ensureFreshSession } from '$lib/utils/openFile'

  type Mode = 'edit' | 'preview' | 'split'

  let {
    filePath,
    projectPath,
    projectId,
    gitRef,
    refLabel
  }: {
    filePath: string
    projectPath: string
    projectId: string
    /** If set, load content from this git ref (read-only). */
    gitRef?: string
    /** Display label for the snapshot (e.g. "abc1234"). */
    refLabel?: string
  } = $props()

  let isReadonly = $derived(!!gitRef)

  let content = $state('')
  let editContent = $state('')
  let loading = $state(true)
  let saving = $state(false)
  let error = $state<string | null>(null)
  let dirty = $derived(!isReadonly && editContent !== content)

  let isMarkdown = $derived(isMarkdownFile(filePath))
  let isBinary = $derived(isBinaryFile(filePath))
  let language = $derived(filePathToLanguage(filePath))
  let mode = $state<Mode>('split')

  // Debounce preview updates in split mode so the markdown parser doesn't
  // re-run on every keystroke.
  let debouncedEdit = $state('')
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    if (debounceTimer) clearTimeout(debounceTimer)
    const snapshot = editContent
    debounceTimer = setTimeout(() => {
      debouncedEdit = snapshot
    }, 150)
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer)
    }
  })

  let previewSource = $derived(mode === 'preview' ? content : debouncedEdit)

  async function load() {
    loading = true
    error = null
    try {
      if (isBinaryFile(filePath)) {
        content = ''
        editContent = ''
        debouncedEdit = ''
      } else if (gitRef) {
        content = await backend.editor.showFile(projectPath, gitRef, filePath)
        editContent = content
        debouncedEdit = content
      } else {
        content = await backend.files.read(projectPath, filePath)
        editContent = content
        debouncedEdit = content
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      loading = false
    }
  }

  async function save() {
    if (!dirty || isReadonly) return
    saving = true
    error = null
    try {
      await backend.files.write(projectPath, filePath, editContent)
      content = editContent
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      saving = false
    }
  }

  async function openInFresh() {
    try {
      await ensureFreshSession(projectId)
      await backend.editor.openFile(projectId, projectPath, filePath)
    } catch (e) {
      console.error('Failed to open in Fresh:', e)
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault()
      save()
    }
  }

  function handleEditorChange(value: string) {
    if (!isReadonly) editContent = value
  }

  // Re-load when filePath or gitRef changes (including initial mount)
  $effect(() => {
    void filePath
    void gitRef
    untrack(() => {
      mode = isMarkdownFile(filePath) ? 'split' : 'edit'
      load()
    })
  })
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex h-full flex-col overflow-hidden" onkeydown={handleKeydown}>
  <ContentToolbar>
    {#snippet left()}
      <span class="truncate text-muted">
        {filePath}
        {#if refLabel}
          <span class="ml-1 text-accent">({refLabel})</span>
        {/if}
        {#if isReadonly}
          <span class="ml-1 text-subtle">read-only</span>
        {/if}
      </span>
    {/snippet}
    {#snippet right()}
      {#if isMarkdown && !isReadonly}
        <TabsRoot
          value={mode}
          onValueChange={(v) => {
            mode = v as Mode
            if (v === 'split') {
              debouncedEdit = editContent
            }
          }}
        >
          <TabsList>
            <TabsTrigger value="edit">Edit</TabsTrigger>
            <TabsTrigger value="split">Split</TabsTrigger>
            <TabsTrigger value="preview">Preview</TabsTrigger>
          </TabsList>
        </TabsRoot>
      {/if}

      {#if dirty}
        <TooltipRoot>
          <TooltipTrigger>
            {#snippet child({ props })}
              <Button variant="ghost" size="xs" onclick={save} disabled={saving} {...props}>
                {saving ? 'Saving...' : 'Save'}
              </Button>
            {/snippet}
          </TooltipTrigger>
          <TooltipContent>
            Save <kbd class="ml-2 font-mono text-[0.68rem] text-subtle">Ctrl+S</kbd>
          </TooltipContent>
        </TooltipRoot>
      {/if}

      {#if !isReadonly}
        <IconButton tooltip="Open in Fresh" onclick={openInFresh}>
          <img src="/svg/fresh.svg" alt="Fresh" width={14} height={14} class="opacity-60" />
        </IconButton>
      {/if}
    {/snippet}
  </ContentToolbar>

  {#if error}
    <div class="px-3 py-2 text-[0.75rem] text-danger">{error}</div>
  {/if}

  <!-- Content -->
  <div class="min-h-0 flex-1">
    {#if loading}
      <div class="px-4 py-3 text-[0.75rem] text-subtle">Loading&hellip;</div>
    {:else if isBinary}
      <div class="flex h-full items-center justify-center text-[0.82rem] text-subtle">
        Binary file &mdash; cannot display
      </div>
    {:else if isMarkdown && mode === 'preview'}
      <div class="h-full overflow-y-auto">
        <MarkdownRenderer source={previewSource} />
      </div>
    {:else if isMarkdown && mode === 'split'}
      <ResizablePaneGroup direction="horizontal">
        <ResizablePane defaultSize={50} minSize={20}>
          <MonacoEditor
            value={editContent}
            {language}
            readonly={isReadonly}
            wordWrap={true}
            onchange={handleEditorChange}
          />
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane defaultSize={50} minSize={20}>
          <div class="h-full overflow-y-auto border-l border-edge">
            <MarkdownRenderer source={previewSource} />
          </div>
        </ResizablePane>
      </ResizablePaneGroup>
    {:else}
      <!-- Full-bleed editor for code files or markdown in edit-only mode -->
      <MonacoEditor
        value={editContent}
        {language}
        readonly={isReadonly}
        wordWrap={isMarkdown}
        onchange={handleEditorChange}
      />
    {/if}
  </div>
</div>
