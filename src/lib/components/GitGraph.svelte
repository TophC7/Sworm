<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { CommitDetail, CommitFileChange } from '$lib/types/backend'
  import { computeGraph, computeRowRender, SWIMLANE_HEIGHT, CIRCLE_RADIUS } from '$lib/graph'
  import type { GraphRow } from '$lib/graph'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import { gitStatusColor, gitStatusDisplay, gitStatusLabel } from '$lib/utils/gitStatus'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import { SvelteSet } from 'svelte/reactivity'

  let {
    projectPath,
    onFileClick,
    onPersistTab
  }: {
    projectPath: string
    onFileClick?: (hash: string, shortHash: string, message: string, filePath: string) => void
    onPersistTab?: () => void
  } = $props()

  let rows = $state<GraphRow[]>([])
  let renders = $derived(rows.map(computeRowRender))
  let currentPath = ''

  // Expanded commit state
  let expandedHash = $state<string | null>(null)
  let expandedDetail = $state<CommitDetail | null>(null)
  let expandedTree = $state<FileTreeNode<CommitFileChange>[]>([])
  let collapsedDirs = new SvelteSet<string>()

  $effect(() => {
    const path = projectPath
    currentPath = path
    expandedHash = null
    expandedDetail = null
    void loadGraph(path)
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

    const detail = await backend.git.getCommitDetail(projectPath, hash)
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

  function formatRef(ref: string): { label: string; kind: 'head' | 'remote' | 'tag' | 'branch' } {
    if (ref.startsWith('HEAD -> ')) return { label: ref.slice(8), kind: 'head' }
    if (ref.startsWith('tag: ')) return { label: ref.slice(5), kind: 'tag' }
    if (ref.includes('/')) return { label: ref, kind: 'remote' }
    return { label: ref, kind: 'branch' }
  }

  function visibleRefs(refs: string[]): string[] {
    return refs.filter((r) => r !== 'HEAD')
  }

  function refClass(kind: 'head' | 'remote' | 'tag' | 'branch'): string {
    switch (kind) {
      case 'head':
        return 'bg-accent/20 text-accent'
      case 'branch':
        return 'bg-accent/10 text-accent-dim'
      case 'remote':
        return 'bg-success/10 text-success'
      case 'tag':
        return 'bg-warning/10 text-warning'
    }
  }
</script>

<div class="flex h-full flex-col text-[0.78rem]">
  <div class="flex shrink-0 items-center justify-between px-2.5 py-1.5">
    <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Graph</span>
  </div>

  {#if rows.length === 0}
    <div class="px-2.5 py-2 text-[0.75rem] text-subtle">No commits found.</div>
  {:else}
    <div class="flex-1 overflow-y-auto">
      {#each rows as row, i (row.commit.hash)}
        {@const r = renders[i]}
        {@const refs = visibleRefs(row.commit.refs)}
        {@const isExpanded = expandedHash === row.commit.hash}

        <!-- Commit row -->
        <button
          class="group flex w-full items-center border-t border-edge/30 text-left hover:bg-raised/50 {isExpanded
            ? 'bg-raised/60'
            : ''}"
          style="height: {SWIMLANE_HEIGHT}px"
          onclick={() => toggleCommit(row.commit.hash)}
        >
          <svg class="shrink-0" width={r.width} height={SWIMLANE_HEIGHT} viewBox="0 0 {r.width} {SWIMLANE_HEIGHT}">
            {#each r.paths as p, pi (pi)}
              <path d={p.d} stroke={p.color} fill="none" stroke-width="1" stroke-linecap="round" />
            {/each}
            {#if r.circle.isMerge}
              <circle cx={r.circle.cx} cy={r.circle.cy} r={CIRCLE_RADIUS + 2} fill={r.circle.color} stroke="none" />
              <circle cx={r.circle.cx} cy={r.circle.cy} r={CIRCLE_RADIUS - 1} fill={r.circle.color} stroke="none" />
            {:else}
              <circle cx={r.circle.cx} cy={r.circle.cy} r={r.circle.r} fill={r.circle.color} stroke="none" />
            {/if}
          </svg>
          <div class="flex min-w-0 flex-1 items-center gap-1.5 pr-2">
            {#if refs.length > 0}
              {#each refs as ref (ref)}
                {@const f = formatRef(ref)}
                <span
                  class="inline-flex max-w-20 shrink-0 items-center truncate rounded px-1 py-px font-mono text-[0.62rem] leading-tight {refClass(
                    f.kind
                  )}"
                  title={f.label}
                >
                  {f.label}
                </span>
              {/each}
            {/if}
            <span class="min-w-0 truncate text-[0.72rem] text-fg">{row.commit.message}</span>
            <span class="ml-auto shrink-0 font-mono text-[0.62rem] text-muted">{row.commit.short_hash}</span>
          </div>
        </button>

        <!-- Expanded file tree (inline under this commit) -->
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
                onFileClick={(node) => expandedHash && node.change && handleFileClick(expandedHash, node.change.path)}
                onFileDblClick={() => onPersistTab?.()}
              >
                {#snippet fileTrailing(node: FileTreeNode<CommitFileChange>)}
                  {#if node.change}
                    <span
                      class="shrink-0 pr-1 font-mono text-[0.62rem] font-bold {gitStatusColor(node.change.status)}"
                      title={gitStatusLabel(node.change.status)}
                    >
                      {gitStatusDisplay(node.change.status)}
                    </span>
                  {/if}
                {/snippet}
              </FileTreeItems>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>
