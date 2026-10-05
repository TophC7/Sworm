<script lang="ts">
  import { untrack } from 'svelte'
  import type { editor } from 'monaco-editor'
  import { platform } from '$lib/platform'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import {
    DialogRoot,
    DialogContent,
    DialogHeader,
    DialogTitle,
    DialogDescription,
    DialogFooter
  } from '$lib/components/ui/dialog'
  import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import MonacoEditor from '$lib/features/editor/renderers/monaco/text/MonacoEditor.svelte'
  import { retainedTextModelBase } from '$lib/features/editor/renderers/monaco/text/modelCache'
  import { filePathToLanguage, isBinaryFile, isMarkdownFile, mediaKind } from '$lib/features/editor/languageMap'
  import { lspSelectorsMatch } from '$lib/features/editor/lsp/registry'
  import { basename, dirname, toProjectRelativePath } from '$lib/utils/paths'
  import MarkdownRenderer from '$lib/components/markdown/MarkdownRenderer.svelte'
  import MediaViewer from '$lib/features/workbench/surfaces/text/MediaViewer.svelte'
  import { watchFileDir } from '$lib/features/files/explorer.svelte'
  import {
    approveLargeTextFile,
    isLargeTextFileApproved,
    clearTextSurfaceDirtyIfClosed,
    discardTextSurfaceBuffer,
    getTextBaseVersion,
    isTextSurfaceDirty,
    markTextSurfaceSaved,
    setTextBaseVersion,
    setTextSurfaceDirty
  } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { promoteTab, renameTextTab } from '$lib/features/workbench/state.svelte'
  import { closeTabWithChecks } from '$lib/features/workbench/tabActions.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
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

  const WHOLE_FILE_LIMIT = 16 * 1024 * 1024
  const STREAM_FILE_LIMIT = 256 * 1024 * 1024
  let streamed = $state(false)
  let largeFileSize = $state<number | null>(null)
  let readBlocked = $state(false)
  let progress = $state<{ bytes: number; total: number } | null>(null)
  let activeStream: string | null = null
  // Metadata identity of the last streamed read; a watcher event that leaves it
  // unchanged (a sibling changed) must not re-stream up to 256 MiB.
  let streamedStatVersion: string | null = null
  let isReadonly = $derived(!!gitRef || streamed)
  let isUntitled = $derived(filePath == null)

  let content = $state('')
  let editContent = $state('')
  // Version of the bytes `content` was read from. Passed back on save so a
  // write that would clobber someone else's change is refused instead.
  // `null` for untitled buffers and snapshots — nothing to check against.
  let diskVersion = $state<string | null>(null)
  let loading = $state(true)
  let saving = $state(false)
  let error = $state<string | null>(null)
  // A save the backend refused because the file moved underneath us — its
  // version changed, or it was deleted — holding what it was trying to write
  // until the user picks a resolution.
  let conflict = $state<{ targetRel: string; content: string; kind: 'conflict' | 'deleted' } | null>(null)
  // Untitled buffers are dirty as soon as they contain any text; without
  // this we'd treat "empty unsaved file" as clean and silently drop it
  // on tab close.
  let dirty = $derived(!isReadonly && (isUntitled ? editContent.length > 0 : editContent !== content))

  // Language/markdown detection keys off filePath. For untitled buffers
  // there's no extension yet, so `plaintext` is the honest default.
  let isMarkdown = $derived(!streamed && filePath != null && isMarkdownFile(filePath))
  let isBinary = $derived(filePath != null && isBinaryFile(filePath))
  // Git snapshots have no on-disk path for asset:// to fetch, so media
  // preview is gated to live (non-gitRef) files.
  let mediaKindValue = $derived(filePath != null && !gitRef ? mediaKind(filePath) : null)
  let language = $derived(filePath != null ? filePathToLanguage(filePath) : 'plaintext')
  let isNix = $derived(language === 'nix')
  let modelKey = $derived(filePath != null && !gitRef && !streamed ? `${folderPath}/${filePath}` : null)
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

  // Bumped by every read of this surface. A read whose token went stale lost
  // its race — a newer load started, or the tab was rebound to another path —
  // and must not apply its bytes over whatever replaced it.
  let readToken = 0

  function cancelRead() {
    ++readToken
    const requestId = activeStream
    activeStream = null
    progress = null
    if (requestId) void backend.files.cancelReadStream(requestId).catch(() => {})
  }

  function formatSize(size: number) {
    return `${(size / (1024 * 1024)).toFixed(1)} MiB (${size.toLocaleString()} bytes)`
  }

  async function readDisk(token: number, folder: string, target: string, approved = streamed, watchReload = false) {
    // One trip for the common case: only a file over the whole-file ceiling
    // pays for the stat that the approval prompt and the stream need.
    const whole = streamed
      ? null
      : await backend.files.read(folder, target).catch((e: unknown) => {
          if ((e as { kind?: unknown } | null)?.kind !== 'tooLarge') throw e
          return null
        })
    if (token !== readToken) return null
    if (watchReload && (saving || dirty || retainedDirtyPending || conflict !== null)) return null
    if (whole) return whole
    const stat = await backend.files.stat(folder, target)
    if (token !== readToken) return null
    if (watchReload && (saving || dirty || retainedDirtyPending || conflict !== null)) return null
    if (watchReload && streamed && stat.version === streamedStatVersion) return null
    if (!stat.regular || stat.size > STREAM_FILE_LIMIT) {
      readBlocked = true
      throw new Error(
        !stat.regular ? 'Cannot open a non-regular file.' : `File is ${formatSize(stat.size)}; the maximum is 256 MiB.`
      )
    }
    if (stat.size <= WHOLE_FILE_LIMIT && !streamed) return backend.files.read(folder, target)
    if (!approved) {
      largeFileSize = stat.size
      readBlocked = true
      return null
    }
    const requestId = crypto.randomUUID()
    let stop: (() => void) | undefined
    try {
      stop = await backend.files.onReadProgress((event) => {
        if (
          token === readToken &&
          activeStream === requestId &&
          event.requestId === requestId &&
          event.folderPath === folder &&
          event.filePath === target
        ) {
          progress = { bytes: event.bytes, total: event.total }
        }
      })
      // Cancellation while the listener is being installed must not start a stream.
      if (token !== readToken) return null
      activeStream = requestId
      progress = { bytes: 0, total: stat.size }
      const file = await backend.files.readStream(requestId, folder, target, stat.version, stat.size)
      if (token !== readToken) return null
      discardTextSurfaceBuffer({ id: tabId, folderPath: folder, filePath: target })
      approveLargeTextFile(tabId, folder, target)
      streamed = true
      streamedStatVersion = stat.version
      mode = 'edit'
      return file
    } finally {
      stop?.()
      if (activeStream === requestId) {
        activeStream = null
        progress = null
      }
    }
  }

  function applyRead(token: number, value: string, version: string | null) {
    if (token !== readToken) return
    readBlocked = false
    error = null
    largeFileSize = null
    content = value
    editContent = value
    debouncedEdit = value
    diskVersion = version
  }

  async function load(approved = false) {
    cancelRead()
    const token = readToken
    const target = filePath
    loading = true
    error = null
    diskVersion = null
    readBlocked = false
    largeFileSize = null
    try {
      if (target == null || isBinaryFile(target)) {
        // Untitled buffer or binary file: nothing to read.
        applyRead(token, '', null)
      } else if (gitRef) {
        applyRead(token, await backend.git.showFile(folderPath, gitRef, target), null)
      } else {
        const retainedBaseContent = retainedDirtyPending ? retainedTextModelBase(folderPath, target) : null
        if (retainedBaseContent !== null) {
          applyRead(token, retainedBaseContent, getTextBaseVersion(folderPath, target))
          return
        }
        const file = await readDisk(token, folderPath, target, approved || streamed)
        if (!file || token !== readToken) return
        // A buffer with unsaved edits keeps the version those edits were based
        // on — retained in the Monaco model across an unmount, or still live
        // here after an external rename. Saving them against the version just
        // read would replace an external change instead of prompting for it.
        const retainedBase = dirty || retainedDirtyPending ? getTextBaseVersion(folderPath, target) : null
        applyRead(token, file.content, retainedBase ?? file.version)
        if (retainedBase == null) setTextBaseVersion(folderPath, target, file.version)
      }
    } catch (e) {
      if (token !== readToken) return
      readBlocked = true
      error = getErrorMessage(e)
    } finally {
      if (token === readToken) loading = false
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
      const native = platform.native
      if (!native) {
        error = 'Save As is unavailable on this platform.'
        return
      }
      saving = true
      try {
        const chosen = await native.dialogs.saveAs({ title: 'Save file', defaultPath: folderPath })
        if (!chosen) {
          saving = false
          return
        }
        const rel = toProjectRelativePath(folderPath, chosen)
        if (!rel) {
          error = 'File must be saved inside the folder.'
          saving = false
          return
        }
        targetRel = rel
      } catch (e) {
        error = getErrorMessage(e)
        saving = false
        return
      }
    } else {
      targetRel = filePath
      saving = true
    }

    await persist(targetRel, editContent, filePath == null ? null : diskVersion)
  }

  /**
   * The write itself. `expectedVersion` null overwrites unconditionally —
   * untitled save-as (the OS dialog already confirmed any replacement) and
   * the explicit overwrite out of the conflict prompt.
   */
  async function persist(targetRel: string, savedContent: string, expectedVersion: string | null) {
    error = null
    try {
      if (isReadonly) {
        throw new Error('Cannot write read-only files.')
      }
      diskVersion = await backend.files.write(folderPath, targetRel, savedContent, expectedVersion)
      markTextSurfaceSaved(folderPath, targetRel, savedContent, diskVersion)
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
      const kind = saveFailureKind(e)
      if (kind) {
        // The file changed or vanished since we read it. Keep the buffer dirty
        // and hold the write until the user picks a resolution.
        conflict = { targetRel, content: savedContent, kind }
        return
      }
      error = getErrorMessage(e)
    } finally {
      saving = false
    }
  }

  /**
   * `conflict` = the on-disk version moved, `deleted` = the file is gone.
   * Both arrive as `{ kind }` from the backend and both are resolvable by
   * writing with a null expected version.
   */
  function saveFailureKind(value: unknown): 'conflict' | 'deleted' | null {
    if (typeof value !== 'object' || value === null || !('kind' in value)) return null
    const kind = (value as { kind?: unknown }).kind
    return kind === 'conflict' || kind === 'deleted' ? kind : null
  }

  /** Take the on-disk version, dropping the unsaved edits. */
  async function reloadFromDisk() {
    const target = filePath
    const pending = conflict
    if (target == null || pending == null) return
    cancelRead()
    const token = readToken
    error = null
    try {
      const file = await readDisk(token, folderPath, target)
      if (!file) {
        if (token === readToken && conflict === pending) conflict = null
        return
      }
      if (token !== readToken || filePath !== target || conflict !== pending) return
      // Keep Monaco mounted: load() would reattach its retained dirty buffer.
      retainedDirtyPending = false
      applyRead(token, file.content, file.version)
      markTextSurfaceSaved(folderPath, target, file.content, file.version)
      conflict = null
    } catch (e) {
      if (token !== readToken || conflict !== pending) return
      error = getErrorMessage(e)
      conflict = null
    }
  }

  /** Keep the editor's version, replacing whatever is on disk. */
  async function overwriteOnDisk() {
    const pending = conflict
    conflict = null
    if (!pending || saving) return
    saving = true
    await persist(pending.targetRel, pending.content, null)
  }

  /**
   * An external change to this file. Clean buffers follow disk; dirty ones keep
   * their edits *and* their now-stale `diskVersion`, so the next save is
   * refused and the conflict prompt — not this reload — decides what wins.
   */
  async function reloadOnDiskChange() {
    const target = filePath
    if (target == null) return
    // `saving` and `conflict` are the windows where a write already owns the
    // version; `retainedDirtyPending` means unsaved edits are still parked in
    // the retained Monaco model and have not reached `editContent` yet.
    if (loading || saving || dirty || retainedDirtyPending || conflict !== null) return
    cancelRead()
    const token = readToken
    try {
      const file = await readDisk(token, folderPath, target, streamed, true)
      if (!file) return
      // Our own save is the common case: the version already matches.
      if (token !== readToken || file.version === diskVersion) return
      // Typing during the read wins; its edits are now based on the old bytes,
      // and the stale `diskVersion` they carry is what makes the save prompt.
      if (filePath !== target || saving || dirty || conflict !== null) return
      applyRead(token, file.content, file.version)
      markTextSurfaceSaved(folderPath, target, file.content, file.version)
    } catch (e) {
      // A delete or mid-write replace fails briefly and the next event re-reads;
      // only a failure that blocked the view needs explaining.
      if (token === readToken && readBlocked) error = getErrorMessage(e)
    }
  }

  // An open file follows disk. The watcher only reports directories this
  // window asked for, so register this file's own: the explorer's set covers
  // it only while the sidebar happens to render that directory.
  $effect(() => {
    const folder = folderPath
    const dir = filePath != null && !gitRef && !isBinary && mediaKindValue == null ? dirname(filePath) : null
    if (dir == null) return
    const unwatch = watchFileDir(folder, dir)
    let disposed = false
    let unlisten: (() => void) | null = null
    void backend.files
      .onChanged((event) => {
        if (disposed) return
        if (event.folder_path !== folder || !event.dirs.includes(dir)) return
        void reloadOnDiskChange()
      })
      .then((stop) => {
        // Disposed before the subscription resolved: drop it immediately.
        if (disposed) stop()
        else unlisten = stop
      })
      .catch(() => {})
    return () => {
      disposed = true
      unlisten?.()
      unlisten = null
      unwatch()
    }
  })

  async function shouldUseLegacyNixLint(target: string): Promise<boolean> {
    try {
      const servers = await backend.lsp.listServers(folderPath)
      const hasConnectedNixLsp = servers.some(
        (entry) =>
          entry.config.enabled &&
          entry.server.status === 'connected' &&
          lspSelectorsMatch(entry.server.document_selectors, 'nix', basename(target))
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
      setTimeout(() => {
        promoteTab(id)
      }, 0)
    }
  }

  // Re-load when filePath or gitRef changes (including initial mount)
  $effect(() => {
    void filePath
    void folderPath
    void gitRef
    untrack(() => {
      streamed = !gitRef && filePath !== null && isLargeTextFileApproved(tabId, folderPath, filePath)
      mode = filePath != null && isMarkdownFile(filePath) ? 'split' : 'edit'
      lintDiagnostics = []
      load()
    })
    return () => {
      cancelRead()
    }
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

{#snippet editor(wordWrap: boolean, onready?: (instance: editor.IStandaloneCodeEditor) => void | (() => void))}
  {#key `${modelKey ?? `untitled:${tabId}`}:${language}`}
    <MonacoEditor
      {tabId}
      largeFile={streamed}
      value={editContent}
      {language}
      readonly={isReadonly}
      {locked}
      {wordWrap}
      onchange={handleEditorChange}
      {filePath}
      {folderPath}
      lspEnabled={!isReadonly}
      {gitDiffRevision}
      {onready}
    />
  {/key}
{/snippet}

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

      {#if dirty && (!isUntitled || platform.native)}
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
    {:else if loading || progress}
      <div class="flex flex-col items-start gap-2 px-4 py-3 text-sm text-muted">
        {#if progress}
          <span role="status">Reading {formatSize(progress.bytes)} of {formatSize(progress.total)}</span>
          <progress class="w-full accent-accent" value={progress.bytes} max={progress.total}></progress>
          <Button
            size="sm"
            onclick={() => {
              cancelRead()
              loading = false
              readBlocked = true
              error = 'File read cancelled.'
              void closeTabWithChecks(tabId)
            }}>Cancel</Button
          >
        {:else}
          <span>Loading&hellip;</span>
        {/if}
      </div>
    {:else if largeFileSize !== null}
      <div class="flex flex-col items-start gap-3 px-4 py-3 text-sm text-muted">
        <p>File is {formatSize(largeFileSize)}. Large files open read-only with language features disabled.</p>
        <Button size="sm" onclick={() => void load(true)}>Open Anyway (Read-Only)</Button>
      </div>
    {:else if readBlocked}
      <div class="px-4 py-3 text-sm text-muted">File not loaded.</div>
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
          {@render editor(true, onSplitEditorReady)}
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
      {@render editor(isMarkdown)}
    {/if}
  </div>
</div>

<DialogRoot
  open={conflict !== null}
  onOpenChange={(open) => {
    // Dismissing keeps the buffer dirty; nothing is written either way.
    if (!open) conflict = null
  }}
>
  <DialogContent>
    {#if conflict}
      <DialogHeader>
        <DialogTitle>{conflict.kind === 'deleted' ? 'File Deleted on Disk' : 'File Changed on Disk'}</DialogTitle>
        <DialogDescription>
          <span class="font-mono text-fg">{basename(conflict.targetRel)}</span>
          {#if conflict.kind === 'deleted'}
            was deleted on disk after it was opened. Recreate writes the unsaved edits in this editor back to that path.
          {:else}
            changed on disk after it was opened. Reload discards the unsaved edits in this editor. Overwrite replaces
            the file on disk with them.
          {/if}
        </DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="outline" onclick={() => (conflict = null)}>Cancel</Button>
        {#if conflict.kind === 'deleted'}
          <Button onclick={() => void overwriteOnDisk()}>Recreate</Button>
        {:else}
          <Button onclick={() => void reloadFromDisk()}>Reload</Button>
          <Button variant="destructive" onclick={() => void overwriteOnDisk()}>Overwrite</Button>
        {/if}
      </DialogFooter>
    {/if}
  </DialogContent>
</DialogRoot>
