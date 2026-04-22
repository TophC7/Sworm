<script lang="ts">
  // Multi-file diff shell. Owns: scroll container, expand/collapse
  // state, DiffModelStore lifecycle, and global Monaco settings
  // (propagated to the pool).

  import { onMount, untrack } from 'svelte'
  import type { FileDiff } from '$lib/types/backend'
  import ContentToolbar from '$lib/components/layout/ContentToolbar.svelte'
  import DiffControls from '$lib/features/workbench/surfaces/diff/DiffControls.svelte'
  import DiffStackFile from '$lib/features/workbench/surfaces/diff/DiffStackFile.svelte'
  import {
    setDiffScrollContext,
    type DiffScrollState
  } from '$lib/features/editor/renderers/monaco/diff/scrollContext.svelte'
  import { getDiffEditorPool } from '$lib/features/editor/renderers/monaco/diff/editorPool.svelte'
  import { DiffModelStore } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'

  let {
    files,
    loading = false,
    initialFile = null,
    scrollNonce = 0,
    label = '',
    idPrefix = 'diff',
    projectId = '',
    projectPath = '',
    commitHash = null,
    stashIndex = null
  }: {
    files: FileDiff[]
    loading?: boolean
    initialFile?: string | null
    scrollNonce?: number
    label?: string
    idPrefix?: string
    projectId?: string
    projectPath?: string
    commitHash?: string | null
    stashIndex?: number | null
  } = $props()

  let expandedFiles = $state<Set<string>>(new Set())
  // Split (side-by-side) is the default — matches the prior UI.
  let sideBySide = $state(true)
  let wrap = $state(false)
  let fontSize = $state(13)

  const store = new DiffModelStore()
  const pool = getDiffEditorPool()

  // Load Monaco once, attach to the store, apply initial settings.
  // `ready()` is idempotent and cached, so multiple DiffStack mounts
  // share the single Monaco module load.
  let storeReady = $state(false)
  onMount(() => {
    let disposed = false
    void (async () => {
      const monaco = await pool.ready()
      if (disposed) return
      store.attach(monaco)
      // Reflect initial Svelte state onto the pool so already-pooled
      // editors pick up settings before their first acquire.
      pool.updateSettings({
        renderSideBySide: sideBySide,
        wordWrap: wrap,
        fontSize
      })
      // Initial sync of the current file list.
      store.sync(files)
      storeReady = true
    })()
    return () => {
      disposed = true
      store.dispose()
    }
  })

  // Keep the model store in sync with the file list.
  $effect(() => {
    if (!storeReady) return
    store.sync(files)
  })

  // Propagate settings changes to the entire pool — both the editors
  // currently mounted and the warm-but-parked ones. A row that reopens
  // later will get the new settings for free.
  $effect(() => {
    pool.updateSettings({ renderSideBySide: sideBySide, wordWrap: wrap, fontSize })
  })

  let totalAdditions = $derived(files.reduce((s, f) => s + (f.additions ?? 0), 0))
  let totalDeletions = $derived(files.reduce((s, f) => s + (f.deletions ?? 0), 0))

  // Initial load opens exactly one row: the explicitly targeted file,
  // or the first file in the scoped diff when no target was provided.
  // Incremental updates preserve the user's existing expand/collapse
  // choices instead of re-expanding rows automatically.
  let lastFileListKey = ''
  $effect(() => {
    const paths = files.map((f) => f.path)
    const key = paths.join('\0')
    if (key === lastFileListKey) return

    const isInitialLoad = lastFileListKey === ''
    lastFileListKey = key

    if (isInitialLoad) {
      const firstPath = initialFile && paths.includes(initialFile) ? initialFile : (paths[0] ?? null)
      expandedFiles = firstPath ? new Set([firstPath]) : new Set()
    } else {
      const currentPaths = new Set(paths)
      const prev = new Set(expandedFiles)
      for (const p of prev) {
        if (!currentPaths.has(p)) prev.delete(p)
      }
      expandedFiles = prev
    }
  })

  // Scroll to the `initialFile` when it flips. Expand it first so the
  // row has height to scroll to.
  let lastScrollRequest = ''
  $effect(() => {
    const file = initialFile
    const requestKey = file ? `${scrollNonce}:${file}` : ''
    if (file && requestKey !== lastScrollRequest && files.length > 0) {
      lastScrollRequest = requestKey
      untrack(() => scrollToFile(file))
    }
  })

  function toggleFile(path: string) {
    const next = new Set(expandedFiles)
    if (next.has(path)) next.delete(path)
    else next.add(path)
    expandedFiles = next
  }

  function scrollToFile(path: string) {
    if (!expandedFiles.has(path)) {
      expandedFiles = new Set([...expandedFiles, path])
    }
    requestAnimationFrame(() => {
      const el = document.getElementById(`${idPrefix}-${path}`)
      el?.scrollIntoView({ behavior: 'smooth', block: 'start' })
    })
  }

  let anyExpanded = $derived(expandedFiles.size > 0)

  function toggleAll() {
    expandedFiles = anyExpanded ? new Set() : new Set(files.map((f) => f.path))
  }

  // --- Scroll context for the IntersectionObserver inside MonacoDiffBody ---
  let scrollEl = $state<HTMLElement | null>(null)

  let scrollCtx: DiffScrollState = $state({
    element: null,
    scrollTop: 0,
    containerHeight: 0
  })

  setDiffScrollContext(scrollCtx)

  $effect(() => {
    const el = scrollEl
    if (!el) return

    scrollCtx.element = el
    scrollCtx.containerHeight = el.clientHeight
    scrollCtx.scrollTop = el.scrollTop

    let ticking = false
    function onScroll() {
      if (!ticking) {
        ticking = true
        requestAnimationFrame(() => {
          if (!el) {
            ticking = false
            return
          }
          const top = el.scrollTop
          if (top !== scrollCtx.scrollTop) scrollCtx.scrollTop = top
          ticking = false
        })
      }
    }

    el.addEventListener('scroll', onScroll, { passive: true })
    // Deferred to the next frame so that downstream consumers' reactive
    // updates don't run synchronously inside the observer callback (the
    // "ResizeObserver loop completed with undelivered notifications"
    // warning fires when they do).
    let roPending = false
    const ro = new ResizeObserver(() => {
      if (roPending) return
      roPending = true
      requestAnimationFrame(() => {
        roPending = false
        const h = el.clientHeight
        if (h !== scrollCtx.containerHeight) scrollCtx.containerHeight = h
      })
    })
    ro.observe(el)

    return () => {
      el.removeEventListener('scroll', onScroll)
      ro.disconnect()
    }
  })
</script>

{#if files.length === 0 && !loading}
  <div class="flex h-full items-center justify-center text-base text-subtle">No changes.</div>
{:else}
  <ContentToolbar>
    {#snippet left()}
      {#if label}
        <span class="font-semibold text-fg">{label}</span>
      {/if}
      <span class="text-muted">
        {files.length} file{files.length !== 1 ? 's' : ''}
      </span>
      {#if totalAdditions > 0}
        <span class="font-mono text-success">+{totalAdditions}</span>
      {/if}
      {#if totalDeletions > 0}
        <span class="font-mono text-danger">-{totalDeletions}</span>
      {/if}
      {#if totalAdditions + totalDeletions > 0}
        {@const addPct = Math.round((totalAdditions / (totalAdditions + totalDeletions)) * 100)}
        <div class="h-1.5 w-20 overflow-hidden rounded-full bg-raised">
          <div class="h-full bg-success" style="width: {addPct}%"></div>
        </div>
      {/if}
    {/snippet}
    {#snippet right()}
      <DiffControls bind:sideBySide bind:wrap bind:fontSize {anyExpanded} onToggleAll={toggleAll} />
    {/snippet}
  </ContentToolbar>

  <div class="min-h-0 flex-1 overflow-y-auto" bind:this={scrollEl}>
    {#each files as file (file.path)}
      {@const expanded = expandedFiles.has(file.path)}
      <DiffStackFile
        {file}
        {expanded}
        {loading}
        {storeReady}
        {store}
        {idPrefix}
        {projectId}
        {projectPath}
        {commitHash}
        {stashIndex}
        onToggle={toggleFile}
      />
    {/each}
  </div>
{/if}
