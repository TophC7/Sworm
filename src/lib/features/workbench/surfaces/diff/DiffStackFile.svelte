<!--
  @component
  Single file row in a diff stack: collapsible header + lazy Monaco body.
-->

<script lang="ts">
  import type { FileDiff } from '$lib/types/backend'
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
  import { gitStatusColor, gitStatusDisplay, gitStatusLabel } from '$lib/features/git/gitStatus'

  interface Props {
    file: FileDiff
    expanded: boolean
    loading?: boolean
    storeReady: boolean
    store: DiffModelStore
    idPrefix?: string
    projectId?: string
    projectPath?: string
    workingStaged?: boolean | null
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
    workingStaged = null,
    commitHash = null,
    stashIndex = null,
    onToggle
  }: Props = $props()

  let statusLetter = $derived(gitStatusDisplay(file.status))
  let statusColor = $derived(gitStatusColor(file.status))
  let statusLabel = $derived(gitStatusLabel(file.status))

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
  // component. Initial capture is correct. `hasExpandedUnchanged`
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
  // must snap back to mirror the preference. Otherwise the next expand
  // would start with a stale "drifted" flag and show the wrong toggle icon.
  $effect(() => {
    if (!expanded) hasExpandedUnchanged = !hideUnchanged
  })
</script>

{#snippet headerAction(Icon: typeof Eye, label: string, onclick: () => void)}
  <TooltipRoot delayDuration={300}>
    <TooltipTrigger
      class="rounded p-1 text-muted transition-colors hover:bg-accent/15 hover:text-fg focus-visible:shadow-focus-ring focus-visible:outline-none"
      {onclick}
    >
      <Icon size={12} />
    </TooltipTrigger>
    <TooltipContent sideOffset={4}>{label}</TooltipContent>
  </TooltipRoot>
{/snippet}

<div id="{idPrefix}-{file.path}" class="border-b border-edge">
  <div class="sticky top-0 z-20 flex w-full items-center border-b border-edge/50 bg-raised">
    <button
      class="flex min-w-0 flex-1 items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-overlay focus-visible:shadow-focus-ring focus-visible:outline-none"
      onclick={() => onToggle(file.path)}
    >
      <ChevronRight size={12} class="shrink-0 text-muted transition-transform {expanded ? 'rotate-90' : ''}" />
      <span class="text-2xs font-bold {statusColor}" title={statusLabel}>{statusLetter}</span>
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
           View control: it changes what you see inside this diff,
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
        gitActionContext={projectId && projectPath && workingStaged !== null
          ? { projectId, projectPath, staged: workingStaged, status: file.status }
          : null}
        onExpandedUnchangedChange={handleExpandedUnchangedChange}
      />
    {:else}
      <div class="px-4 py-6 text-center text-sm text-subtle">No diff available</div>
    {/if}
  {/if}
</div>
