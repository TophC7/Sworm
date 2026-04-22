<!--
  @component
  Single file row in a diff stack: collapsible header + lazy Monaco body.
-->

<script lang="ts">
  import type { FileDiff, GitStatusKind } from '$lib/types/backend'
  import type { DiffModelStore } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'
  import {
    openCommitSnapshot,
    openCurrentFileFromDiff,
    openStashSnapshot
  } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import MonacoDiffBody from '$lib/features/editor/renderers/monaco/diff/MonacoDiffBody.svelte'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import { Separator } from '$lib/components/ui/separator'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import { ChevronRight, ChevronsDownUp, ChevronsUpDown, SquareArrowOutUpRight, Eye } from '$lib/icons/lucideExports'

  interface Props {
    file: FileDiff
    expanded: boolean
    loading?: boolean
    storeReady: boolean
    store: DiffModelStore
    idPrefix?: string
    projectId?: string
    projectPath?: string
    commitHash?: string | null
    stashIndex?: number | null
    onToggle: (path: string) => void
  }

  let {
    file,
    expanded,
    loading = false,
    storeReady,
    store,
    idPrefix = 'diff',
    projectId = '',
    projectPath = '',
    commitHash = null,
    stashIndex = null,
    onToggle
  }: Props = $props()

  const STATUS_DISPLAY: Record<GitStatusKind, { color: string; letter: string; label: string }> = {
    added: { color: 'text-success', letter: 'A', label: 'Added' },
    modified: { color: 'text-accent', letter: 'M', label: 'Modified' },
    deleted: { color: 'text-danger', letter: 'D', label: 'Deleted' },
    renamed: { color: 'text-accent', letter: 'R', label: 'Renamed' },
    copied: { color: 'text-accent', letter: 'C', label: 'Copied' },
    untracked: { color: 'text-success', letter: '?', label: 'Untracked' },
    unmerged: { color: 'text-warning', letter: 'U', label: 'Unmerged' },
    unknown: { color: 'text-muted', letter: ' ', label: 'Unknown' }
  }
  let statusDisplay = $derived(STATUS_DISPLAY[file.status])

  function openInEditor(filePath: string) {
    if (!projectId || !projectPath) return
    openCurrentFileFromDiff(projectId, filePath)
  }

  function viewAtCommit(filePath: string) {
    if (!projectId || !commitHash) return
    openCommitSnapshot(projectId, filePath, commitHash)
  }

  function viewAtStash(filePath: string) {
    if (!projectId || stashIndex == null) return
    openStashSnapshot(projectId, filePath, stashIndex)
  }

  // Per-file "expand all unchanged code" toggle. Seeded from the store
  // (preference survives collapse/scroll) and mirrored back on toggle.
  // `file.path` is the `{#each}` key, so a path change recreates this
  // component — initial-capture is correct. `hasExpandedUnchanged`
  // tracks Monaco's live state (may drift from the preference when the
  // user expands a region inside the editor). The command-seq is bumped
  // on toggle so MonacoDiffBody's effect re-fires even when the boolean
  // is unchanged but Monaco drifted.
  function seedHide(): boolean {
    return store.get(file.path)?.hideUnchanged ?? true
  }
  let hideUnchanged = $state(seedHide())
  let hasExpandedUnchanged = $state(!seedHide())
  let hideUnchangedCommandSeq = $state(0)

  function toggleHideUnchanged() {
    const next = hasExpandedUnchanged
    hideUnchanged = next
    hasExpandedUnchanged = !next
    store.persistHideUnchangedPreference(file.path, next)
    hideUnchangedCommandSeq += 1
  }

  function handleExpandedUnchangedChange(next: boolean) {
    hasExpandedUnchanged = next
  }

  // Reset the drift tracker when Monaco detaches. Once the row collapses
  // there is no live editor to report hidden-area state, so the tracker
  // must snap back to mirror the preference — otherwise the next expand
  // would start with a stale "drifted" flag and show the wrong toggle icon.
  $effect(() => {
    if (!expanded) hasExpandedUnchanged = !hideUnchanged
  })
</script>

{#snippet headerAction(Icon: typeof Eye, label: string, onclick: () => void)}
  <TooltipRoot delayDuration={300}>
    <TooltipTrigger class="rounded p-1 text-muted transition-colors hover:bg-accent/15 hover:text-fg" {onclick}>
      <Icon size={12} />
    </TooltipTrigger>
    <TooltipContent sideOffset={4}>{label}</TooltipContent>
  </TooltipRoot>
{/snippet}

<div id="{idPrefix}-{file.path}" class="border-b border-edge">
  <div class="sticky top-0 z-20 flex w-full items-center border-b border-edge/50 bg-raised/90 backdrop-blur-sm">
    <button
      class="flex min-w-0 flex-1 items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-raised"
      onclick={() => onToggle(file.path)}
    >
      <ChevronRight size={12} class="shrink-0 text-muted transition-transform {expanded ? 'rotate-90' : ''}" />
      <span class="text-2xs font-bold {statusDisplay.color}" title={statusDisplay.label}>{statusDisplay.letter}</span>
      <FileIcon filename={file.path} size={13} />
      <span class="min-w-0 truncate font-mono text-sm text-fg">
        {#if file.oldPath}<span class="text-muted">{file.oldPath} → </span>{/if}{file.path}
      </span>
      <span class="ml-auto shrink-0 font-mono text-2xs">
        {#if (file.additions ?? 0) > 0}<span class="text-success">+{file.additions}</span>{/if}
        {#if (file.deletions ?? 0) > 0}<span class="ml-1 text-danger">-{file.deletions}</span>{/if}
      </span>
    </button>
    <div class="flex shrink-0 items-center gap-0.5 pr-2">
      <!-- Per-file expand/collapse of Monaco's unchanged-region
           folding. Lives on the left of the divider because it's a
           VIEW control — it changes what you see INSIDE this diff,
           not where the file opens. Hidden until the row is expanded,
           since it has no effect on a collapsed body. -->
      {#if expanded && storeReady && !file.binary}
        {@render headerAction(
          hasExpandedUnchanged ? ChevronsDownUp : ChevronsUpDown,
          hasExpandedUnchanged ? 'Collapse unchanged code' : 'Expand all unchanged code',
          toggleHideUnchanged
        )}
      {/if}
      {#if projectId}
        {#if expanded && storeReady && !file.binary}
          <Separator orientation="vertical" class="mx-1 h-4" />
        {/if}
        {#if commitHash}
          {@render headerAction(Eye, 'View at commit', () => viewAtCommit(file.path))}
          {#if file.status !== 'deleted'}
            {@render headerAction(SquareArrowOutUpRight, 'Open current file', () => openInEditor(file.path))}
          {/if}
        {:else if stashIndex != null}
          {@render headerAction(Eye, 'View in stash', () => viewAtStash(file.path))}
          {#if file.status !== 'deleted'}
            {@render headerAction(SquareArrowOutUpRight, 'Open current file', () => openInEditor(file.path))}
          {/if}
        {:else}
          {@render headerAction(SquareArrowOutUpRight, 'Open in editor', () => openInEditor(file.path))}
        {/if}
      {/if}
    </div>
  </div>

  {#if expanded}
    {#if loading}
      <div class="px-4 py-6 text-center text-sm text-subtle">Loading diff...</div>
    {:else if !storeReady}
      <div class="px-4 py-6 text-center text-sm text-subtle">Loading editor...</div>
    {:else if store.get(file.path)}
      <MonacoDiffBody
        path={file.path}
        {store}
        {hideUnchanged}
        {hideUnchangedCommandSeq}
        onExpandedUnchangedChange={handleExpandedUnchangedChange}
      />
    {:else}
      <div class="px-4 py-6 text-center text-sm text-subtle">No diff available</div>
    {/if}
  {/if}
</div>
