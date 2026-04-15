<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { CommitDetail, CommitFileChange } from '$lib/types/backend'
  import { computeGraph, computeRowRender, SWIMLANE_HEIGHT, CIRCLE_RADIUS } from '$lib/utils/graph'
  import type { GraphRow } from '$lib/utils/graph'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import GitStatusBadge from '$lib/components/git/GitStatusBadge.svelte'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import GitStashList from '$lib/components/git/GitStashList.svelte'
  import { refLabel, visibleRefs } from '$lib/utils/gitRefs'
  import CommitTooltip from '$lib/components/CommitTooltip.svelte'
  import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'
  import { IconButton } from '$lib/components/ui/button'
  import { GitGraphIcon, PackageIcon } from '$lib/icons/lucideExports'
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'

  let {
    projectPath,
    onFileClick,
    onStashFileClick,
    onPersistTab,
    onMutate
  }: {
    projectPath: string
    onFileClick?: (hash: string, shortHash: string, message: string, filePath: string) => void
    onStashFileClick?: (stashIndex: number, message: string, filePath: string) => void
    onPersistTab?: () => void
    onMutate?: () => void
  } = $props()

  let activeTab = $state<'graph' | 'stashes'>('graph')
  let rows = $state<GraphRow[]>([])
  let renders = $derived(rows.map(computeRowRender))
  let currentPath = ''
  let stashCount = $state(0)

  // Expanded commit state
  let expandedHash = $state<string | null>(null)
  let expandedDetail = $state<CommitDetail | null>(null)
  let expandedTree = $state<FileTreeNode<CommitFileChange>[]>([])
  let collapsedDirs = new SvelteSet<string>()

  // Shared detail cache (tooltip prefetch + expand reuse the same data)
  let detailCache = new SvelteMap<string, CommitDetail>()

  $effect(() => {
    const path = projectPath
    if (path === currentPath) return
    currentPath = path
    expandedHash = null
    expandedDetail = null
    detailCache.clear()
    void loadGraph(path)
    void loadStashCount(path)
  })

  async function loadGraph(path: string) {
    try {
      const commits = await backend.git.getGraph(path, 100)
      if (path !== currentPath) return
      rows = computeGraph(commits)
    } catch {
      if (path === currentPath) rows = []
    }
  }

  async function loadStashCount(path: string) {
    try {
      const count = await backend.git.stashCount(path)
      if (path !== currentPath) return
      stashCount = count
    } catch {
      if (path === currentPath) stashCount = 0
    }
  }

  /** Fetch commit detail, returning from cache when available. */
  async function fetchDetail(hash: string): Promise<CommitDetail | null> {
    const cached = detailCache.get(hash)
    if (cached) return cached
    const detail = await backend.git.getCommitDetail(projectPath, hash)
    if (detail) detailCache.set(hash, detail)
    return detail
  }

  /** Prefetch detail on hover so the tooltip opens with data ready. */
  function prefetchDetail(hash: string) {
    if (!detailCache.has(hash)) void fetchDetail(hash)
  }

  async function toggleCommit(hash: string) {
    if (expandedHash === hash) {
      expandedHash = null
      expandedDetail = null
      expandedTree = []
      return
    }

    expandedHash = hash
    expandedDetail = null
    expandedTree = []
    collapsedDirs.clear()

    const detail = await fetchDetail(hash)
    if (expandedHash !== hash) return
    expandedDetail = detail
    if (detail) {
      expandedTree = buildFileTree(detail.files)
    }
  }

  function handleFileClick(hash: string, filePath: string) {
    if (!expandedDetail) return
    onFileClick?.(hash, expandedDetail.short_hash, expandedDetail.message, filePath)
  }

  function toggleDir(path: string) {
    if (collapsedDirs.has(path)) collapsedDirs.delete(path)
    else collapsedDirs.add(path)
  }

  /** Map branch names to their graph lane colors (first occurrence wins). */
  let branchColorMap = $derived.by(() => {
    const map = new Map<string, string>()
    for (let i = 0; i < rows.length; i++) {
      const r = renders[i]
      for (const rawRef of visibleRefs(rows[i].commit.refs)) {
        const name = refLabel(rawRef)
        if (!map.has(name)) map.set(name, r.circle.color)
      }
    }
    return map
  })

  function handleStashMutate() {
    void loadStashCount(projectPath)
    onMutate?.()
  }
</script>

<div class="flex h-full flex-col text-[0.78rem]">
  <div class="flex shrink-0 items-center justify-between px-2.5 py-1.5">
    <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">
      {activeTab === 'graph' ? 'Graph' : 'Stashes'}{activeTab === 'stashes' && stashCount > 0 ? ` (${stashCount})` : ''}
    </span>
    <div class="flex items-center gap-0.5">
      <IconButton
        tooltip="Commit graph"
        tooltipSide="bottom"
        class="rounded p-0.5 transition-colors {activeTab === 'graph' ? 'text-fg' : 'text-muted hover:text-fg'}"
        onclick={() => (activeTab = 'graph')}
      >
        <GitGraphIcon size={13} />
      </IconButton>
      <IconButton
        tooltip="Stashes{stashCount > 0 ? ` (${stashCount})` : ''}"
        tooltipSide="bottom"
        class="rounded p-0.5 transition-colors {activeTab === 'stashes' ? 'text-fg' : 'text-muted hover:text-fg'}"
        onclick={() => (activeTab = 'stashes')}
      >
        <PackageIcon size={13} />
      </IconButton>
    </div>
  </div>

  {#if activeTab === 'graph'}
    {#if rows.length === 0}
      <div class="px-2.5 py-2 text-[0.75rem] text-subtle">No commits found.</div>
    {:else}
      <TooltipProvider delayDuration={400} skipDelayDuration={100}>
        <div class="flex-1 overflow-y-auto">
          {#each rows as row, i (row.commit.hash)}
            {@const r = renders[i]}
            {@const refs = visibleRefs(row.commit.refs)}
            {@const isExpanded = expandedHash === row.commit.hash}

            <TooltipRoot>
              <TooltipTrigger
                class="group flex w-full items-center border-t border-edge/30 text-left hover:bg-raised/50 {isExpanded
                  ? 'bg-raised/60'
                  : ''}"
                style="height: {SWIMLANE_HEIGHT}px"
                onclick={() => toggleCommit(row.commit.hash)}
                onmouseenter={() => prefetchDetail(row.commit.hash)}
              >
                <svg
                  class="shrink-0"
                  width={r.width}
                  height={SWIMLANE_HEIGHT}
                  viewBox="0 0 {r.width} {SWIMLANE_HEIGHT}"
                >
                  {#each r.paths as p, pi (pi)}
                    <path d={p.d} stroke={p.color} fill="none" stroke-width="1" stroke-linecap="round" />
                  {/each}
                  {#if r.circle.isMerge}
                    <circle
                      cx={r.circle.cx}
                      cy={r.circle.cy}
                      r={CIRCLE_RADIUS + 2}
                      fill={r.circle.color}
                      stroke="none"
                    />
                    <circle
                      cx={r.circle.cx}
                      cy={r.circle.cy}
                      r={CIRCLE_RADIUS - 1}
                      fill={r.circle.color}
                      stroke="none"
                    />
                  {:else}
                    <circle cx={r.circle.cx} cy={r.circle.cy} r={r.circle.r} fill={r.circle.color} stroke="none" />
                  {/if}
                </svg>
                <div class="flex min-w-0 flex-1 items-center gap-1.5 pr-2">
                  <span class="min-w-0 truncate text-[0.72rem] text-fg">{row.commit.message}</span>
                  {#if refs.length > 0}
                    <!-- First ref: full label badge, colored to match the graph line -->
                    <span
                      class="ml-auto inline-flex max-w-24 shrink-0 items-center truncate rounded px-1 py-px font-mono text-[0.62rem] leading-tight"
                      style="background: {r.circle.color}20; color: {r.circle.color}"
                      title={refLabel(refs[0])}
                    >
                      {refLabel(refs[0])}
                    </span>
                    {#each refs.slice(1) as ref (ref)}
                      <span
                        class="size-3 shrink-0 rounded-sm"
                        style="background: {r.circle.color}40"
                        title={refLabel(ref)}
                      ></span>
                    {/each}
                  {/if}
                </div>
              </TooltipTrigger>
              <TooltipContent class="max-w-135" sideOffset={6} side="right" align="center">
                <CommitTooltip
                  commit={row.commit}
                  detail={detailCache.get(row.commit.hash) ?? null}
                  graphColor={r.circle.color}
                />
              </TooltipContent>
            </TooltipRoot>

            {#if isExpanded}
              <div class="border-t border-edge/30 bg-surface/40 py-1">
                {#if !expandedDetail}
                  <div class="px-4 py-1.5 text-[0.68rem] text-subtle">Loading files...</div>
                {:else if expandedTree.length === 0}
                  <div class="px-4 py-1.5 text-[0.68rem] text-subtle">No files changed.</div>
                {:else}
                  <FileTreeItems
                    nodes={expandedTree}
                    isCollapsed={(path) => collapsedDirs.has(path)}
                    onToggleDir={toggleDir}
                    onFileClick={(node) =>
                      expandedHash && node.change && handleFileClick(expandedHash, node.change.path)}
                    onFileDblClick={() => onPersistTab?.()}
                  >
                    {#snippet fileTrailing(node: FileTreeNode<CommitFileChange>)}
                      {#if node.change}
                        <GitStatusBadge status={node.change.status} />
                      {/if}
                    {/snippet}
                  </FileTreeItems>
                {/if}
              </div>
            {/if}
          {/each}
        </div>
      </TooltipProvider>
    {/if}
  {:else}
    <GitStashList
      {projectPath}
      {branchColorMap}
      onMutate={handleStashMutate}
      onFileClick={onStashFileClick}
      {onPersistTab}
    />
  {/if}
</div>
