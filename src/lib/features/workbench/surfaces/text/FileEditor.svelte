<script lang="ts">
  import { untrack } from 'svelte'
  import type { editor } from 'monaco-editor'
  import { save as saveDialog } from '@tauri-apps/plugin-dialog'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import MonacoEditor from '$lib/features/editor/renderers/monaco/text/MonacoEditor.svelte'
  import { filePathToLanguage, isBinaryFile, isMarkdownFile, mediaKind } from '$lib/features/editor/languageMap'
  import { basename } from '$lib/utils/paths'
  import MarkdownRenderer from '$lib/components/markdown/MarkdownRenderer.svelte'
  import MediaViewer from '$lib/features/workbench/surfaces/text/MediaViewer.svelte'
  import {
    clearTextSurfaceDirtyIfClosed,
    discardTextSurfaceBuffer,
    isTextSurfaceDirty,
    markTextSurfaceSaved,
    setTextSurfaceDirty
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { promoteTab, renameTextTab } from '$lib/features/workbench/state.svelte'
  import { getGitSummary } from '$lib/features/git/state.svelte'

  type Mode = 'edit' | 'preview' | 'split'

  let {
    tabId,
    filePath,
    folderPath,
    gitRef,
    refLabel,
    initialTemporary = false,
    locked = false
  }: {
    tabId: string
    /** `null` = unsaved "Untitled" buffer. First save triggers save-as. */
    filePath: string | null
    folderPath: string
    /** If set, load content from this git ref (read-only). */
    gitRef?: string
    /** Display label for the snapshot (e.g. "abc1234"). */
    refLabel?: string
    initialTemporary?: boolean
    locked?: boolean
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
  // Git snapshots have no on-disk path for asset:// to fetch, so media
  // preview is gated to live (non-gitRef) files.
  let mediaKindValue = $derived(filePath != null && !gitRef ? mediaKind(filePath) : null)
  let language = $derived(filePath != null ? filePathToLanguage(filePath) : 'plaintext')
  let isNix = $derived(language === 'nix')
  let lspUriPath = $derived(filePath != null && !gitRef ? `${folderPath}/${filePath}` : null)
  let gitSummary = $derived(getGitSummary(folderPath))
  let gitDiffRevision = $derived(
    filePath == null || gitRef
      ? ''
      : (gitSummary?.changes ?? [])
          .filter((change) => change.path === filePath)
          .map((change) => `${change.status}:${change.staged}:${change.additions ?? ''}:${change.deletions ?? ''}`)
          .join('|')
  )
  let mode = $state<Mode>('split')
  let syncScroll = $state(false)
  let splitEditor = $state<editor.IStandaloneCodeEditor | null>(null)
  let splitPreview = $state<HTMLDivElement | null>(null)

  function onSplitEditorReady(instance: editor.IStandaloneCodeEditor) {
    splitEditor = instance
    return () => {
      splitEditor = null
    }
  }

  $effect(() => {
    if (!syncScroll || mode !== 'split' || !splitEditor || !splitPreview) return
    const editor = splitEditor
    const preview = splitPreview
    let updatingEditor = false
    let expectedPreviewTop = -1

    function syncFromEditor() {
      if (updatingEditor) return
      const range = editor.getScrollHeight() - editor.getLayoutInfo().height
      if (range <= 0) return
      const progress = Math.max(0, Math.min(1, editor.getScrollTop() / range))
      preview.scrollTop = progress * Math.max(0, preview.scrollHeight - preview.clientHeight)
      // DOM scroll events arrive later; ignore the echo, including pixel rounding.
      expectedPreviewTop = preview.scrollTop
    }

    function syncFromPreview() {
      if (Math.abs(preview.scrollTop - expectedPreviewTop) < 1) return
      const range = preview.scrollHeight - preview.clientHeight
      if (range <= 0) return
      const progress = Math.max(0, Math.min(1, preview.scrollTop / range))
      updatingEditor = true
      try {
        editor.setScrollTop(progress * Math.max(0, editor.getScrollHeight() - editor.getLayoutInfo().height))
      } finally {
        updatingEditor = false
      }
    }

    const scrollListener = editor.onDidScrollChange((event) => {
      if (event.scrollTopChanged || event.scrollHeightChanged) syncFromEditor()
    })
    preview.addEventListener('scroll', syncFromPreview, { passive: true })
    // Re-align after pane resizing, async markdown rendering, or image loading.
    const resizeObserver = new ResizeObserver(syncFromEditor)
    resizeObserver.observe(preview)
    const markdownBody = preview.querySelector('.markdown-body')
    if (markdownBody) resizeObserver.observe(markdownBody)
    syncFromEditor()
    return () => {
      scrollListener.dispose()
      preview.removeEventListener('scroll', syncFromPreview)
      resizeObserver.disconnect()
    }
  })

  // Debounce preview updates in split mode so the markdown parser doesn't
  // re-run on every keystroke.
  let debouncedEdit = $state('')
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  // Inactive dirty tabs remount from the retained Monaco model; don't
  // clear their dirty flag before the model has had a chance to reattach.
  let retainedDirtyPending = $state(false)
  let promotedOnEdit = $state(false)
  let promoteTimer: ReturnType<typeof setTimeout> | null = null
  $effect.pre(() => {
    const id = tabId
    untrack(() => {
      retainedDirtyPending = isTextSurfaceDirty(id)
    })
  })
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
        content = await backend.editor.showFile(folderPath, gitRef, filePath)
        editContent = content
        debouncedEdit = content
      } else {
        content = await backend.files.read(folderPath, filePath)
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
        const chosen = await saveDialog({ title: 'Save file', defaultPath: folderPath })
        if (!chosen) {
          saving = false
          return
        }
        // Guard against sibling directories whose path happens to share
        // the folder root as a prefix (`/home/a/proj-backup/...` vs
        // `/home/a/proj`). Require an exact match OR a `/` boundary.
        const inside = chosen === folderPath || chosen.startsWith(folderPath + '/')
        if (!inside) {
          error = 'File must be saved inside the folder.'
          saving = false
          return
        }
        // backend.files.write takes a folder-relative path.
        targetRel = chosen.slice(folderPath.length).replace(/^\/+/, '')
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
      const savedContent = editContent
      await backend.files.write(folderPath, targetRel, savedContent)
      markTextSurfaceSaved(folderPath, targetRel, savedContent)
      content = savedContent
      if (filePath == null) {
        discardTextSurfaceBuffer({ id: tabId, folderPath, filePath: null })
        // Promote the tab to the real path. The filePath effect will
        // reload, but editContent already matches so no flash.
        renameTextTab(tabId, targetRel)
      }
      if (isNix) {
        if (await shouldUseLegacyNixLint(targetRel)) {
          await lintNix()
        } else {
          lintDiagnostics = []
        }
      }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      saving = false
    }
  }

  async function shouldUseLegacyNixLint(target: string): Promise<boolean> {
    try {
      const servers = await backend.lsp.listServers(folderPath)
      const fileName = basename(target)
      const extension = normalizeExtension(target.includes('.') ? target.slice(target.lastIndexOf('.')) : '')

      const hasConnectedNixLsp = servers.some(
        (entry) =>
          entry.config.enabled &&
          entry.server.status === 'connected' &&
          entry.server.document_selectors.some(
            (selector) =>
              selector.language === 'nix' ||
              selector.filenames.includes(fileName) ||
              (extension.length > 0 && selector.extensions.some((value) => normalizeExtension(value) === extension))
          )
      )

      return !hasConnectedNixLsp
    } catch (e) {
      console.warn('nix-lsp-status:', e)
      return true
    }
  }

  async function lintNix() {
    const target = filePath
    if (target == null) return
    try {
      const diagnostics = await backend.nix.lint(folderPath, target)
      if (filePath !== target) return
      lintDiagnostics = diagnostics
    } catch (e) {
      console.warn('nix-lint:', e)
      if (filePath === target) lintDiagnostics = []
    }
  }

  function normalizeExtension(value: string): string {
    const trimmed = value.trim().toLowerCase()
    if (!trimmed) return ''
    return trimmed.startsWith('.') ? trimmed : `.${trimmed}`
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault()
      void save()
    }
  }

  function handleEditorChange(value: string) {
    retainedDirtyPending = false
    if (isReadonly) return
    editContent = value
    if (initialTemporary && !promotedOnEdit) {
      promotedOnEdit = true
      const id = tabId
      // Keep the first edit's Monaco update isolated from the workbench
      // commit that flips preview chrome to persistent chrome.
      promoteTimer = setTimeout(() => {
        promoteTimer = null
        promoteTab(id)
      }, 0)
    }
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

  // Mirror local dirty state into the workbench-level registry so the
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
    const id = tabId
    return () => {
      clearTextSurfaceDirtyIfClosed(id)
    }
  })
  $effect(() => {
    if (dirty) {
      retainedDirtyPending = false
      setTextSurfaceDirty(tabId, true)
      return
    }
    if (retainedDirtyPending) return
    setTextSurfaceDirty(tabId, dirty)
  })
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex h-full flex-col overflow-hidden" onkeydown={handleKeydown}>
  <PanelHeader class="text-sm">
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
        <Button
          variant={syncScroll ? 'accent' : 'ghost'}
          size="xs"
          aria-pressed={syncScroll}
          disabled={mode !== 'split'}
          title="Sync relative scroll positions in Split view"
          onclick={() => (syncScroll = !syncScroll)}>Sync</Button
        >
      {/if}

      {#if dirty}
        {#if isMarkdown && !isReadonly}
          <Separator orientation="vertical" class="mx-0.5 h-4" />
        {/if}
        <TooltipRoot>
          <TooltipTrigger onclick={save} disabled={saving}>
            {#snippet child({ props })}
              <Button variant="ghost" size="xs" {...props} disabled={saving}>
                {saving ? 'Saving...' : 'Save'}
              </Button>
            {/snippet}
          </TooltipTrigger>
          <TooltipContent>
            Save <kbd class="ml-2 font-mono text-xs text-subtle">Ctrl+S</kbd>
          </TooltipContent>
        </TooltipRoot>
      {/if}
    {/snippet}
  </PanelHeader>

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
    {#if mediaKindValue != null && filePath != null}
      <MediaViewer {folderPath} {filePath} kind={mediaKindValue} />
    {:else if loading}
      <div class="px-4 py-3 text-sm text-subtle">Loading&hellip;</div>
    {:else if isBinary}
      <div class="flex h-full items-center justify-center text-base text-subtle">
        Binary file &mdash; cannot display
      </div>
    {:else if isMarkdown && mode === 'preview'}
      <div class="h-full overflow-y-auto">
        <MarkdownRenderer source={previewSource} {folderPath} {filePath} />
      </div>
    {:else if isMarkdown && mode === 'split'}
      <ResizablePaneGroup direction="horizontal">
        <ResizablePane defaultSize={50} minSize={20}>
          {#key `${lspUriPath ?? `untitled:${tabId}`}:${language}`}
            <MonacoEditor
              {tabId}
              value={editContent}
              {language}
              readonly={isReadonly}
              {locked}
              wordWrap={true}
              onchange={handleEditorChange}
              uriPath={lspUriPath}
              {filePath}
              {folderPath}
              lspEnabled={!isReadonly}
              {gitDiffRevision}
              onready={onSplitEditorReady}
            />
          {/key}
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane defaultSize={50} minSize={20}>
          <div bind:this={splitPreview} class="h-full overflow-y-auto border-l border-edge">
            <MarkdownRenderer source={previewSource} {folderPath} {filePath} />
          </div>
        </ResizablePane>
      </ResizablePaneGroup>
    {:else}
      <!-- Full-bleed editor for code files or markdown in edit-only mode -->
      {#key `${lspUriPath ?? `untitled:${tabId}`}:${language}`}
        <MonacoEditor
          {tabId}
          value={editContent}
          {language}
          readonly={isReadonly}
          {locked}
          wordWrap={isMarkdown}
          onchange={handleEditorChange}
          uriPath={lspUriPath}
          {filePath}
          {folderPath}
          lspEnabled={!isReadonly}
          {gitDiffRevision}
        />
      {/key}
    {/if}
  </div>
</div>
