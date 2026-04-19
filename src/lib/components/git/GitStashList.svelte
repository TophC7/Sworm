<script lang="ts">
  import { backend } from '$lib/api/backend'
  import type { CommitFileChange, StashEntry } from '$lib/types/backend'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import { parseStashMessage } from '$lib/utils/git'
  import GitStatusBadge from '$lib/components/git/GitStatusBadge.svelte'
  import { timeAgo, formatFullDate } from '$lib/utils/date'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { TooltipContent, TooltipProvider, TooltipRoot, TooltipTrigger } from '$lib/components/ui/tooltip'
  import { IconButton } from '$lib/components/ui/button'
  import { ClockIcon, Play, Trash2 } from '$lib/icons/lucideExports'
  import { GRAPH_COLORS } from '$lib/utils/graph'
  import { runNotifiedTask } from '$lib/utils/notifiedTask'
  import { SvelteSet } from 'svelte/reactivity'

  let {
    projectPath,
    branchColorMap = new Map(),
    onMutate,
    onFileClick,
    onPersistTab
  }: {
    projectPath: string
    branchColorMap?: Map<string, string>
    onMutate?: () => void
    onFileClick?: (stashIndex: number, message: string, filePath: string) => void
    onPersistTab?: () => void
  } = $props()

  let stashes = $state<StashEntry[]>([])
  let expandedIndex = $state<number | null>(null)
  let expandedTree = $derived.by(() => {
    if (expandedIndex === null) return []
    const entry = stashes.find((s) => s.index === expandedIndex)
    return entry ? buildFileTree(entry.files) : []
  })
  let collapsedDirs = new SvelteSet<string>()
  let currentPath = ''

  // Drop confirmation
  let dropIndex = $state<number | null>(null)
  let showDropConfirm = $derived(dropIndex !== null)

  $effect(() => {
    const path = projectPath
    currentPath = path
    expandedIndex = null
    void loadStashes(path)
  })

  async function loadStashes(path: string) {
    try {
      const result = await backend.git.stashList(path)
      if (path !== currentPath) return
      stashes = result
    } catch {
      if (path === currentPath) stashes = []
    }
  }

  export function reload() {
    void loadStashes(projectPath)
  }

  /** Look up a branch's color from the graph lane assignments. */
  function branchColor(branch: string): string {
    // Try exact match first, then check for origin/ prefix variants
    return (
      branchColorMap.get(branch) ??
      branchColorMap.get(`origin/${branch}`) ??
      branchColorMap.get(branch.replace(/^origin\//, '')) ??
      GRAPH_COLORS[0]
    )
  }

  function toggleStash(index: number) {
    if (expandedIndex === index) {
      expandedIndex = null
      return
    }
    expandedIndex = index
    collapsedDirs.clear()
  }

  function toggleDir(path: string) {
    if (collapsedDirs.has(path)) collapsedDirs.delete(path)
    else collapsedDirs.add(path)
  }

  async function handlePop(index: number) {
    await runNotifiedTask(
      async () => {
        await backend.git.stashPop(projectPath, index)
        expandedIndex = null
        await loadStashes(projectPath)
        onMutate?.()
      },
      {
        loading: { title: 'Applying stash', description: `stash@{${index}}` },
        success: { title: 'Stash applied', description: `stash@{${index}}` },
        error: { title: 'Apply stash failed' }
      }
    )
  }

  async function handleDrop() {
    if (dropIndex === null) return
    const idx = dropIndex
    dropIndex = null
    await runNotifiedTask(
      async () => {
        await backend.git.stashDrop(projectPath, idx)
        if (expandedIndex === idx) expandedIndex = null
        await loadStashes(projectPath)
        onMutate?.()
      },
      {
        loading: { title: 'Dropping stash', description: `stash@{${idx}}` },
        success: { title: 'Stash dropped', description: `stash@{${idx}}` },
        error: { title: 'Drop stash failed' }
      }
    )
  }
</script>

<div class="flex h-full flex-col text-base">
  {#if stashes.length === 0}
    <div class="px-2.5 py-2 text-sm text-subtle">No stashes.</div>
  {:else}
    <TooltipProvider delayDuration={400} skipDelayDuration={100}>
      <div class="flex-1 overflow-y-auto">
        {#each stashes as stash (stash.index)}
          {@const isExpanded = expandedIndex === stash.index}
          {@const parsed = parseStashMessage(stash.message)}
          {@const color = parsed.branch ? branchColor(parsed.branch) : GRAPH_COLORS[0]}
          {@const fileCount = stash.files.length}
          {@const totalAdds = stash.files.reduce((s, f) => s + f.additions, 0)}
          {@const totalDels = stash.files.reduce((s, f) => s + f.deletions, 0)}

          <!-- Row is a wrapping div so the action buttons can sit as siblings
               to the tooltip-triggering row button. Nesting <button>s inside
               <button>s breaks keyboard/pointer semantics. -->
          <div
            class="group flex w-full items-center gap-1.5 border-t border-edge/30 px-2.5 hover:bg-raised/50 {isExpanded
              ? 'bg-raised/60'
              : ''}"
            style="height: 22px"
          >
            <TooltipRoot>
              <TooltipTrigger
                class="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 border-none bg-transparent px-0 text-left outline-none"
                onclick={() => toggleStash(stash.index)}
              >
                <span class="shrink-0 rounded bg-accent-bg px-1 py-px font-mono text-2xs text-accent">
                  {stash.index}
                </span>
                {#if parsed.branch}
                  <span
                    class="inline-flex max-w-24 shrink-0 items-center truncate rounded px-1 py-px font-mono text-2xs leading-tight"
                    style="background: {color}20; color: {color}"
                  >
                    {parsed.branch}
                  </span>
                {/if}
                <span class="min-w-0 flex-1 truncate text-sm text-fg">
                  {parsed.label}
                </span>
              </TooltipTrigger>
              <TooltipContent class="max-w-80" sideOffset={6} side="right" align="center">
                <div class="flex flex-col gap-2 py-0.5">
                  <div class="flex items-center gap-1 text-sm">
                    <ClockIcon size={10} class="text-subtle" />
                    <span class="text-muted">{timeAgo(stash.date)}</span>
                    <span class="text-subtle">({formatFullDate(stash.date)})</span>
                  </div>
                  <p class="text-sm leading-snug text-fg">{stash.message}</p>
                  {#if fileCount > 0}
                    <p class="text-xs">
                      <span class="text-muted">{fileCount} file{fileCount !== 1 ? 's' : ''},</span>
                      {' '}
                      <span class="text-success">{totalAdds} insertions(+)</span>
                      <span class="text-muted">,</span>
                      {' '}
                      <span class="text-danger">{totalDels} deletions(-)</span>
                    </p>
                  {/if}
                  {#if parsed.branch}
                    <div class="flex items-center gap-1 text-xs">
                      <span
                        class="inline-block h-1 w-1 shrink-0 rounded-full"
                        style="background: {color}"
                        aria-hidden="true"
                      ></span>
                      <span class="font-mono" style="color: {color}">{parsed.branch}</span>
                    </div>
                  {/if}
                </div>
              </TooltipContent>
            </TooltipRoot>

            <span class="shrink-0 text-2xs text-subtle group-hover:hidden">
              {timeAgo(stash.date)}
            </span>
            <div class="hidden shrink-0 items-center gap-0.5 group-hover:flex">
              <IconButton tooltip="Pop (apply + remove)" tooltipSide="left" onclick={() => handlePop(stash.index)}>
                <Play size={12} />
              </IconButton>
              <IconButton
                tooltip="Drop stash"
                tooltipSide="left"
                tone="danger"
                onclick={() => (dropIndex = stash.index)}
              >
                <Trash2 size={12} />
              </IconButton>
            </div>
          </div>

          {#if isExpanded}
            <div class="border-t border-edge/30 bg-surface/40 py-1">
              {#if expandedTree.length === 0}
                <div class="px-4 py-1.5 text-xs text-subtle">No files changed.</div>
              {:else}
                <FileTreeItems
                  nodes={expandedTree}
                  isCollapsed={(path) => collapsedDirs.has(path)}
                  onToggleDir={toggleDir}
                  onFileClick={(node) => {
                    if (expandedIndex !== null && node.change) {
                      const entry = stashes.find((s) => s.index === expandedIndex)
                      onFileClick?.(expandedIndex, entry?.message ?? '', node.change.path)
                    }
                  }}
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
</div>

<ConfirmDialog
  open={showDropConfirm}
  title="Drop Stash?"
  message="This will permanently delete this stash entry. This cannot be undone."
  confirmLabel="Drop"
  onConfirm={handleDrop}
  onCancel={() => (dropIndex = null)}
/>
