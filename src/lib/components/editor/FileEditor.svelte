<script lang="ts">
  import { untrack } from 'svelte'
  import { save as saveDialog } from '@tauri-apps/plugin-dialog'
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
  import { runNotifiedTask } from '$lib/utils/notifiedTask'
  import { clearEditorDirty, setEditorDirty } from '$lib/stores/dirtyEditors.svelte'
  import { renameEditorTab } from '$lib/stores/workspace.svelte'

  type Mode = 'edit' | 'preview' | 'split'

  let {
    tabId,
    filePath,
    projectPath,
    projectId,
    gitRef,
    refLabel
  }: {
    tabId: string
    /** `null` = unsaved "Untitled" buffer. First save triggers save-as. */
    filePath: string | null
    projectPath: string
    projectId: string
    /** If set, load content from this git ref (read-only). */
    gitRef?: string
    /** Display label for the snapshot (e.g. "abc1234"). */
    refLabel?: string
  } = $props()

  let isReadonly = $derived(!!gitRef)
  let isUntitled = $derived(filePath == null)

  let content = $state('')
  let editContent = $state('')
  let loading = $state(true)
  let saving = $state(false)
  let error = $state<string | null>(null)
  // Untitled buffers are dirty as soon as they contain any text; without
  // this we'd treat "empty unsaved file" as clean and silently drop it
  // on tab close.
  let dirty = $derived(!isReadonly && (isUntitled ? editContent.length > 0 : editContent !== content))

  // Language/markdown detection keys off filePath. For untitled buffers
  // there's no extension yet, so `plaintext` is the honest default.
  let isMarkdown = $derived(filePath != null && isMarkdownFile(filePath))
  let isBinary = $derived(filePath != null && isBinaryFile(filePath))
  let language = $derived(filePath != null ? filePathToLanguage(filePath) : 'plaintext')
  let isNix = $derived(language === 'nix')
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
      if (filePath == null) {
        // Untitled buffer: start empty, skip backend read entirely.
        content = ''
        editContent = ''
        debouncedEdit = ''
      } else if (isBinaryFile(filePath)) {
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

  let lintDiagnostics = $state<{ message: string; line: number; column: number }[]>([])

  async function save() {
    // Re-entry guard: Ctrl+S held down or clicked twice could fire two
    // concurrent writes. The second would race the first's state
    // updates and could flip `dirty` back to true against stale state.
    if (saving) return
    if (!dirty || isReadonly) return

    // Untitled buffers: prompt for a path via the OS save dialog before
    // writing. On success we rebind the tab to the chosen path — the
    // filePath $effect below will re-run load(), which will read back
    // the just-written file and reconcile content == editContent,
    // flipping dirty=false naturally.
    let targetRel: string
    if (filePath == null) {
      saving = true
      try {
        const chosen = await saveDialog({ title: 'Save file', defaultPath: projectPath })
        if (!chosen) {
          saving = false
          return
        }
        // Guard against sibling directories whose path happens to share
        // the project root as a prefix (`/home/a/proj-backup/...` vs
        // `/home/a/proj`). Require an exact match OR a `/` boundary.
        const inside = chosen === projectPath || chosen.startsWith(projectPath + '/')
        if (!inside) {
          error = 'File must be saved inside the project directory.'
          saving = false
          return
        }
        // backend.files.write takes a project-relative path.
        targetRel = chosen.slice(projectPath.length).replace(/^\/+/, '')
      } catch (e) {
        error = e instanceof Error ? e.message : String(e)
        saving = false
        return
      }
    } else {
      targetRel = filePath
      saving = true
    }

    error = null
    try {
      await backend.files.write(projectPath, targetRel, editContent)
      content = editContent
      if (filePath == null) {
        // Promote the tab to the real path. The filePath effect will
        // reload, but editContent already matches so no flash.
        renameEditorTab(projectId, tabId, targetRel)
      }
      if (isNix) lintNix()
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      saving = false
    }
  }

  async function lintNix() {
    const target = filePath
    if (target == null) return
    try {
      const diagnostics = await backend.nix.lint(projectPath, target)
      if (filePath !== target) return
      lintDiagnostics = diagnostics
    } catch (e) {
      console.warn('nix-lint:', e)
      if (filePath === target) lintDiagnostics = []
    }
  }

  async function openInFresh() {
    const path = filePath
    if (path == null) return
    await runNotifiedTask(
      async () => {
        await ensureFreshSession(projectId)
        await backend.editor.openFile(projectId, projectPath, path)
      },
      {
        loading: { title: 'Opening in Fresh', description: path },
        error: { title: 'Open in Fresh failed' }
      }
    )
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault()
      void save()
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
      mode = filePath != null && isMarkdownFile(filePath) ? 'split' : 'edit'
      lintDiagnostics = []
      load()
    })
  })

  // Mirror local dirty state into the workspace-level registry so the
  // reload / close paths can warn the user about unsaved buffers.
  //
  // Keyed by tabId (not filePath) so untitled buffers — which have no
  // filePath yet — still participate, and so promoting an untitled to
  // a real path doesn't orphan its dirty entry under the stale key.
  //
  // Split into two effects on purpose: a single effect that captured
  // tabId and also depended on `dirty` would run its cleanup on every
  // keystroke — clearing then re-setting the dirty entry — and any
  // $derived reader of the registry would see it flicker off and back
  // on every character typed.
  $effect(() => {
    const project = projectId
    const id = tabId
    return () => clearEditorDirty(project, id)
  })
  $effect(() => {
    setEditorDirty(projectId, tabId, dirty)
  })
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex h-full flex-col overflow-hidden" onkeydown={handleKeydown}>
  <ContentToolbar>
    {#snippet left()}
      <span class="truncate text-muted">
        {filePath ?? 'Untitled'}
        {#if refLabel}
          <span class="ml-1 text-accent">({refLabel})</span>
        {/if}
        {#if isReadonly}
          <span class="ml-1 text-subtle">read-only</span>
        {/if}
        {#if isUntitled}
          <span class="ml-1 text-subtle">(unsaved)</span>
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
            Save <kbd class="ml-2 font-mono text-xs text-subtle">Ctrl+S</kbd>
          </TooltipContent>
        </TooltipRoot>
      {/if}

      {#if !isReadonly && !isUntitled}
        <IconButton tooltip="Open in Fresh" onclick={openInFresh}>
          <img src="/svg/fresh.svg" alt="Fresh" width={14} height={14} class="opacity-60" />
        </IconButton>
      {/if}
    {/snippet}
  </ContentToolbar>

  {#if error}
    <div class="px-3 py-2 text-sm text-danger">{error}</div>
  {/if}
  {#if lintDiagnostics.length > 0}
    <div class="flex flex-col gap-0.5 px-3 py-1.5 text-xs text-warning">
      {#each lintDiagnostics as d}
        <span>Line {d.line}:{d.column} — {d.message}</span>
      {/each}
    </div>
  {/if}

  <!-- Content -->
  <div class="min-h-0 flex-1">
    {#if loading}
      <div class="px-4 py-3 text-sm text-subtle">Loading&hellip;</div>
    {:else if isBinary}
      <div class="flex h-full items-center justify-center text-base text-subtle">
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
