<script lang="ts" module>
  export interface DiffEntry {
    rawDiff: string
    oldContent?: string | null
    newContent?: string | null
  }

  export interface DiffFile {
    path: string
    status: string
    additions: number
    deletions: number
  }
</script>

<script lang="ts">
  import { untrack } from 'svelte'
  import { DiffMode } from '$lib/diff/types'
  import { setDiffScrollContext, type DiffScrollState } from '$lib/diff/scrollContext.svelte'
  import ContentToolbar from '$lib/components/ContentToolbar.svelte'
  import DiffControls from '$lib/components/diff/DiffControls.svelte'
  import DiffStackFile from '$lib/components/diff/DiffStackFile.svelte'

  const AUTO_EXPAND_LIMIT = 8

  let {
    files,
    diffs,
    loading = false,
    initialFile = null,
    label = '',
    idPrefix = 'diff',
    projectId = '',
    projectPath = '',
    commitHash = null,
    stashIndex = null
  }: {
    files: DiffFile[]
    diffs: Map<string, DiffEntry>
    loading?: boolean
    initialFile?: string | null
    label?: string
    idPrefix?: string
    projectId?: string
    projectPath?: string
    commitHash?: string | null
    stashIndex?: number | null
  } = $props()

  let expandedFiles = $state<Set<string>>(new Set())
  let diffMode = $state(DiffMode.Split)
  let diffWrap = $state(false)
  let diffFontSize = $state(13)

  let totalAdditions = $derived(files.reduce((s, f) => s + f.additions, 0))
  let totalDeletions = $derived(files.reduce((s, f) => s + f.deletions, 0))

  // Auto-expand on initial load or when file list changes.
  // Preserves user expand/collapse state on incremental updates (e.g. git poll
  // adding a file) — only new files are added, nothing is collapsed.
  let lastFileListKey = ''
  $effect(() => {
    const paths = files.map((f) => f.path)
    const key = paths.join('\0')
    if (key === lastFileListKey) return

    const isInitialLoad = lastFileListKey === ''
    lastFileListKey = key

    if (isInitialLoad) {
      if (paths.length <= AUTO_EXPAND_LIMIT) {
        expandedFiles = new Set(paths)
      } else if (initialFile) {
        expandedFiles = new Set([initialFile])
      }
    } else {
      // Incremental: add new files to expanded set without clobbering existing state
      const currentPaths = new Set(paths)
      const prev = new Set(expandedFiles)
      // Remove files that no longer exist
      for (const p of prev) {
        if (!currentPaths.has(p)) prev.delete(p)
      }
      // Auto-expand new files if we're still under the limit
      if (currentPaths.size <= AUTO_EXPAND_LIMIT) {
        for (const p of paths) prev.add(p)
      }
      expandedFiles = prev
    }
  })

  // Scroll to initialFile when it changes
  let lastScrolledFile: string | null = null
  $effect(() => {
    const file = initialFile
    if (file && file !== lastScrolledFile && files.length > 0) {
      lastScrolledFile = file
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

  let allExpanded = $derived(files.length > 0 && expandedFiles.size === files.length)

  function toggleAll() {
    expandedFiles = allExpanded ? new Set() : new Set(files.map((f) => f.path))
  }

  // --- Scroll context for pane virtualization ---
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
    const ro = new ResizeObserver(() => {
      const h = el.clientHeight
      if (h !== scrollCtx.containerHeight) scrollCtx.containerHeight = h
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
      <DiffControls
        bind:mode={diffMode}
        bind:wrap={diffWrap}
        bind:fontSize={diffFontSize}
        {allExpanded}
        onToggleAll={toggleAll}
      />
    {/snippet}
  </ContentToolbar>

  <!-- Stacked diffs -->
  <div class="min-h-0 flex-1 overflow-y-auto" bind:this={scrollEl}>
    {#each files as file (file.path)}
      {@const entry = diffs.get(file.path)}
      {@const expanded = expandedFiles.has(file.path)}
      <DiffStackFile
        {file}
        {entry}
        {expanded}
        {diffMode}
        {diffWrap}
        {diffFontSize}
        {loading}
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
