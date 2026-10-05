<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { CommitDetail } from '$lib/types/backend'
  import { computeGraph, computeRowRender } from '$lib/features/git/graph'
  import ChangedFilesTree from '$lib/features/git/ChangedFilesTree.svelte'
  import { openCommitDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import GitStashList from '$lib/features/git/GitStashList.svelte'
  import GitBranches from '$lib/features/git/GitBranches.svelte'
  import { refLabel, visibleRefs } from '$lib/features/git/gitRefs'
  import GitCommitRow from '$lib/features/git/GitCommitRow.svelte'
  import { TooltipProvider } from '$lib/components/ui/tooltip'
  import { IconButton } from '$lib/components/ui/button'
  import { GitBranchPlusIcon, GitGraphIcon, LoaderCircle, PackageIcon } from '$lib/icons/lucideExports'
  import { SvelteMap } from 'svelte/reactivity'
  import * as branches from '$lib/features/git/branches.svelte'
  import { getGitSidebarTab, setGitSidebarTab } from '$lib/features/app-shell/sidebar/state.svelte'
  import { getGitGraph, getStashCount, loadGraph } from '$lib/features/git/state.svelte'
  import { untrack } from 'svelte'

  let { folderPath }: { folderPath: string } = $props()

  let activeTab = $derived(getGitSidebarTab())
  let commits = $derived(getGitGraph(folderPath))
  let rows = $derived(commits ? computeGraph(commits) : [])
  let renders = $derived(rows.map(computeRowRender))
  let stashCount = $derived(getStashCount(folderPath))
  let branchEntry = $derived(branches.byFolder.get(folderPath))

  // Expanded commit state
  let expandedHash = $state<string | null>(null)
  let expandedDetail = $state<CommitDetail | null>(null)

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
      return
    }

    expandedHash = hash
    expandedDetail = null

    const detail = await fetchDetail(hash)
    if (expandedHash !== hash) return
    expandedDetail = detail
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
        active={activeTab === 'graph'}
        onclick={() => setGitSidebarTab('graph')}
      >
        <GitGraphIcon size={13} />
      </IconButton>
      <IconButton
        tooltip="Stashes{stashCount > 0 ? ` (${stashCount})` : ''}"
        active={activeTab === 'stashes'}
        onclick={() => setGitSidebarTab('stashes')}
      >
        <PackageIcon size={13} />
      </IconButton>
      <IconButton
        tooltip="Branches"
        active={activeTab === 'branches'}
        onclick={() => setGitSidebarTab('branches')}
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
                {:else}
                  {@const detail = expandedDetail}
                  <ChangedFilesTree
                    files={detail.files}
                    open={(path) => openCommitDiff(folderPath, row.commit.hash, detail.short_hash, detail.message, path)}
                  />
                {/if}
              </div>
            {/if}
          {/each}
        </div>
      </TooltipProvider>
    {/if}
  {:else if activeTab === 'stashes'}
    <GitStashList {folderPath} {branchColorMap} />
  {:else}
    <GitBranches {folderPath} {branchColorMap} />
  {/if}
</div>
