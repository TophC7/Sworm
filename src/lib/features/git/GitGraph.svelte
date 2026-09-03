<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { TabId } from '$lib/features/workbench/model'
  import type { CommitDetail, CommitFileChange } from '$lib/types/backend'
  import { computeGraph, computeRowRender } from '$lib/features/git/graph'
  import type { GraphRow } from '$lib/features/git/graph'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import GitStashList from '$lib/features/git/GitStashList.svelte'
  import GitBranches from '$lib/features/git/GitBranches.svelte'
  import { refLabel, visibleRefs } from '$lib/features/git/gitRefs'
  import GitCommitRow from '$lib/features/git/GitCommitRow.svelte'
  import { TooltipProvider } from '$lib/components/ui/tooltip'
  import { IconButton } from '$lib/components/ui/button'
  import { GitBranchPlusIcon, GitGraphIcon, LoaderCircle, PackageIcon } from '$lib/icons/lucideExports'
  import { SvelteMap, SvelteSet } from 'svelte/reactivity'
  import * as branches from '$lib/features/git/branches.svelte'
  import { getGitSidebarTab, setGitSidebarTab, type GitSidebarTab } from '$lib/features/app-shell/sidebar/state.svelte'
  import { getGitGraph, getStashCount, loadGraph } from '$lib/features/git/state.svelte'
  import { untrack } from 'svelte'

  let {
    folderPath,
    onFileClick,
    onStashFileClick,
    onPersistTab
  }: {
    folderPath: string
    onFileClick?: (hash: string, shortHash: string, message: string, filePath: string) => TabId | Promise<TabId> | void
    onStashFileClick?: (stashIndex: number, message: string, filePath: string) => TabId | Promise<TabId> | void
    onPersistTab?: (openedTab: TabId | Promise<TabId> | null | undefined) => void
  } = $props()

  let activeTab = $derived(getGitSidebarTab())
  let commits = $derived(getGitGraph(folderPath))
  let rows = $derived(commits ? computeGraph(commits) : [])
  let renders = $derived(rows.map(computeRowRender))
  let stashCount = $derived(getStashCount(folderPath))
  let branchEntry = $derived(branches.byFolder.get(folderPath))

  // Expanded commit state
  let expandedHash = $state<string | null>(null)
  let expandedDetail = $state<CommitDetail | null>(null)
  let expandedTree = $state<FileTreeNode<CommitFileChange>[]>([])
  let collapsedDirs = new SvelteSet<string>()
  let pendingOpenedTab = $state<Promise<TabId> | null>(null)

  // Shared detail cache (tooltip prefetch + expand reuse the same data)
  let detailCache = new SvelteMap<string, CommitDetail>()

  $effect(() => {
    void loadGraph(folderPath)
  })

  // Graph changed: drop expansion and cached details for commits that no
  // longer exist (undo, rebase, folder switch).
  $effect(() => {
    const hashes = new Set(rows.map((row) => row.commit.hash))
    untrack(() => {
      if (expandedHash && !hashes.has(expandedHash)) {
        expandedHash = null
        expandedDetail = null
        expandedTree = []
      }
      for (const hash of [...detailCache.keys()]) {
        if (!hashes.has(hash)) detailCache.delete(hash)
      }
    })
  })

  /** Fetch commit detail, returning from cache when available. */
  async function fetchDetail(hash: string): Promise<CommitDetail | null> {
    const cached = detailCache.get(hash)
    if (cached) return cached
    const detail = await backend.git.getCommitDetail(folderPath, hash)
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
    if (!expandedDetail) {
      pendingOpenedTab = null
      return
    }
    const openedTab = onFileClick?.(hash, expandedDetail.short_hash, expandedDetail.message, filePath)
    pendingOpenedTab = openedTab == null ? null : Promise.resolve(openedTab)
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

  function setActiveTab(tab: GitSidebarTab) {
    setGitSidebarTab(tab)
  }
</script>

<div class="flex h-full flex-col text-base">
  <div class="flex shrink-0 items-center justify-between px-2.5 py-1.5">
    <span class="inline-flex items-center gap-1.5 text-xs font-semibold tracking-wide text-muted uppercase">
      <span>
        {activeTab === 'graph'
          ? 'Graph'
          : activeTab === 'stashes'
            ? `Stashes${stashCount > 0 ? ` (${stashCount})` : ''}`
            : 'Branches'}
      </span>
      {#if activeTab === 'branches' && branchEntry?.fetching}
        <LoaderCircle size={11} class="animate-spin text-muted" aria-label="Fetching branches" />
      {/if}
    </span>
    <div class="flex items-center gap-0.5">
      <IconButton
        tooltip="Commit graph"
        tooltipSide="bottom"
        active={activeTab === 'graph'}
        onclick={() => setActiveTab('graph')}
      >
        <GitGraphIcon size={13} />
      </IconButton>
      <IconButton
        tooltip="Stashes{stashCount > 0 ? ` (${stashCount})` : ''}"
        tooltipSide="bottom"
        active={activeTab === 'stashes'}
        onclick={() => setActiveTab('stashes')}
      >
        <PackageIcon size={13} />
      </IconButton>
      <IconButton
        tooltip="Branches"
        tooltipSide="bottom"
        active={activeTab === 'branches'}
        onclick={() => setActiveTab('branches')}
      >
        <GitBranchPlusIcon size={13} />
      </IconButton>
    </div>
  </div>

  {#if activeTab === 'graph'}
    {#if commits === null}
      <!-- First load in flight; rows arrive from the store. -->
    {:else if rows.length === 0}
      <div class="px-2.5 py-2 text-sm text-subtle">No commits found.</div>
    {:else}
      <TooltipProvider delayDuration={400} skipDelayDuration={100}>
        <div class="flex-1 overflow-y-auto">
          {#each rows as row, i (row.commit.hash)}
            {@const r = renders[i]}
            {@const isExpanded = expandedHash === row.commit.hash}

            <GitCommitRow
              commit={row.commit}
              render={r}
              detail={detailCache.get(row.commit.hash) ?? null}
              graphColor={r.circle.color}
              active={isExpanded}
              onRowClick={toggleCommit}
              onPrefetch={prefetchDetail}
            />

            {#if isExpanded}
              <div class="border-t border-edge/30 bg-surface py-1">
                {#if !expandedDetail}
                  <div class="px-4 py-1.5 text-xs text-subtle">Loading files...</div>
                {:else if expandedTree.length === 0}
                  <div class="px-4 py-1.5 text-xs text-subtle">No files changed.</div>
                {:else}
                  <FileTreeItems
                    nodes={expandedTree}
                    isCollapsed={(node) => collapsedDirs.has(node.path)}
                    onToggleDir={toggleDir}
                    onFileClick={(node) =>
                      expandedHash && node.change && handleFileClick(expandedHash, node.change.path)}
                    onFileDblClick={() => onPersistTab?.(pendingOpenedTab)}
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
  {:else if activeTab === 'stashes'}
    <GitStashList {folderPath} {branchColorMap} onFileClick={onStashFileClick} {onPersistTab} />
  {:else}
    <GitBranches {folderPath} {branchColorMap} />
  {/if}
</div>
