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
  import { DiffModeEnum } from '@git-diff-view/svelte'
  import { gitStatusColor, gitStatusDisplay, gitStatusLabel } from '$lib/utils/gitStatus'
  import DiffViewer from '$lib/components/DiffViewer.svelte'
  import LazyRender from '$lib/components/LazyRender.svelte'
  import DiffControls from '$lib/components/DiffControls.svelte'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import ChevronRight from '@lucide/svelte/icons/chevron-right'

  const AUTO_EXPAND_LIMIT = 8

  let {
    files,
    diffs,
    loading = false,
    initialFile = null,
    label = '',
    idPrefix = 'diff'
  }: {
    files: DiffFile[]
    diffs: Map<string, DiffEntry>
    loading?: boolean
    initialFile?: string | null
    label?: string
    idPrefix?: string
  } = $props()

  let expandedFiles = $state<Set<string>>(new Set())
  let diffMode = $state(DiffModeEnum.Split)
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

  function expandAll() {
    expandedFiles = new Set(files.map((f) => f.path))
  }

  function collapseAll() {
    expandedFiles = new Set()
  }
</script>

{#if files.length === 0 && !loading}
  <div class="flex h-full items-center justify-center text-[0.78rem] text-subtle">No changes.</div>
{:else}
  <!-- Stats bar -->
  <div class="flex shrink-0 items-center gap-3 border-b border-edge bg-surface/50 px-4 py-1.5 text-[0.72rem]">
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
    <span class="ml-auto flex items-center gap-3">
      <span class="flex gap-1">
        <button class="text-muted hover:text-fg" onclick={expandAll}>Expand</button>
        <span class="text-subtle">/</span>
        <button class="text-muted hover:text-fg" onclick={collapseAll}>Collapse</button>
      </span>
      <DiffControls bind:mode={diffMode} bind:wrap={diffWrap} bind:fontSize={diffFontSize} />
    </span>
  </div>

  <!-- Stacked diffs -->
  <div class="min-h-0 flex-1 overflow-y-auto">
    {#each files as file (file.path)}
      {@const entry = diffs.get(file.path)}
      {@const expanded = expandedFiles.has(file.path)}
      <div id="{idPrefix}-{file.path}" class="border-b border-edge">
        <button
          class="sticky top-0 z-10 flex w-full items-center gap-2 border-b border-edge/50 bg-raised/90 px-3 py-1.5 text-left backdrop-blur-sm transition-colors hover:bg-raised"
          onclick={() => toggleFile(file.path)}
        >
          <ChevronRight size={12} class="shrink-0 text-muted transition-transform {expanded ? 'rotate-90' : ''}" />
          <span class="text-[0.65rem] font-bold {gitStatusColor(file.status)}" title={gitStatusLabel(file.status)}
            >{gitStatusDisplay(file.status)}</span
          >
          <FileIcon filename={file.path} size={13} />
          <span class="min-w-0 truncate font-mono text-[0.72rem] text-fg">{file.path}</span>
          <span class="ml-auto shrink-0 font-mono text-[0.62rem]">
            {#if file.additions > 0}<span class="text-success">+{file.additions}</span>{/if}
            {#if file.deletions > 0}<span class="ml-1 text-danger">-{file.deletions}</span>{/if}
          </span>
        </button>

        {#if expanded}
          {#if loading}
            <div class="px-4 py-6 text-center text-[0.72rem] text-subtle">Loading diff...</div>
          {:else if entry}
            <LazyRender minHeight={Math.min(Math.max(80, (file.additions + file.deletions) * 22), 600)}>
              <DiffViewer
                rawDiff={entry.rawDiff}
                filePath={file.path}
                oldContent={entry.oldContent}
                newContent={entry.newContent}
                mode={diffMode}
                wrap={diffWrap}
                fontSize={diffFontSize}
              />
            </LazyRender>
          {:else}
            <div class="px-4 py-6 text-center text-[0.72rem] text-subtle">No diff available</div>
          {/if}
        {/if}
      </div>
    {/each}
  </div>
{/if}
