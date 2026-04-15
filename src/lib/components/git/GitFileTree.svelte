<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import type { GitChange, GitSummary } from '$lib/types/backend'
  import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import {
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator
  } from '$lib/components/ui/dropdown-menu'
  import { ButtonGroup } from '$lib/components/ui/button-group'
  import { IconButton } from '$lib/components/ui/button'
  import { FileDiff, MinusCircle, PlusCircle, Trash2, PackageIcon, ChevronDown } from '$lib/icons/lucideExports'
  import GitStatusBadge from '$lib/components/git/GitStatusBadge.svelte'

  let {
    summary,
    projectPath,
    hasCommits = false,
    onFileClick,
    onPersistTab,
    onViewAllChanges,
    onCommit,
    onStageAll,
    onUnstageAll,
    onDiscardAll,
    onStashAll,
    onUndoLastCommit,
    onPush,
    onPushForceWithLease,
    onPull,
    onFetch
  }: {
    summary: GitSummary
    projectPath: string
    hasCommits?: boolean
    onFileClick?: (filePath: string, staged: boolean) => void
    onPersistTab?: () => void
    onViewAllChanges?: (staged: boolean) => void
    onCommit?: (message: string) => void
    onStageAll?: () => void
    onUnstageAll?: () => void
    onDiscardAll?: () => void
    onStashAll?: () => void
    onUndoLastCommit?: () => void
    onPush?: () => void
    onPushForceWithLease?: () => void
    onPull?: () => void
    onFetch?: () => void
  } = $props()

  let collapsedDirs = new SvelteSet<string>()
  let commitMessage = $state('')
  let committing = $state(false)

  $effect(() => {
    projectPath
    collapsedDirs.clear()
  })

  function getDirKey(section: string, path: string): string {
    return `${section}:${path}`
  }

  function toggleDir(section: string, path: string) {
    const key = getDirKey(section, path)
    if (collapsedDirs.has(key)) collapsedDirs.delete(key)
    else collapsedDirs.add(key)
  }

  let stagedFiles = $derived(summary.changes.filter((c) => c.staged))
  let unstagedFiles = $derived(summary.changes.filter((c) => !c.staged))

  let stagedTree = $derived(buildFileTree(stagedFiles))
  let unstagedTree = $derived(buildFileTree(unstagedFiles))

  let canCommit = $derived(commitMessage.trim().length > 0 && stagedFiles.length > 0 && !committing)

  function handleCommit() {
    if (!canCommit) return
    committing = true
    onCommit?.(commitMessage.trim())
    commitMessage = ''
    committing = false
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault()
      handleCommit()
    }
  }
</script>

<div class="text-[0.78rem]">
  <div class="border-b border-edge px-2.5 py-2">
    <textarea
      class="w-full resize-none rounded border border-edge bg-surface px-2 py-1.5 text-[0.75rem] text-fg placeholder:text-subtle focus:border-accent/50 focus:outline-none"
      rows={2}
      placeholder="Commit message..."
      bind:value={commitMessage}
      onkeydown={handleKeydown}
    ></textarea>
    <div class="mt-1.5">
      <ButtonGroup class="w-full">
        <button
          data-slot="button"
          class="flex-1 rounded border border-edge bg-raised px-2.5 py-1 text-[0.68rem] font-medium text-fg transition-colors hover:border-accent hover:text-bright disabled:cursor-not-allowed disabled:opacity-40"
          disabled={!canCommit}
          onclick={handleCommit}
        >
          Commit{stagedFiles.length > 0 ? ` (${stagedFiles.length})` : ''}
        </button>
        <DropdownMenuRoot>
          <DropdownMenuTrigger
            data-slot="button"
            class="flex items-center rounded border border-edge bg-raised px-1 py-1 text-muted transition-colors hover:border-accent hover:text-bright"
          >
            <ChevronDown size={11} />
          </DropdownMenuTrigger>
          <DropdownMenuContent class="min-w-[180px] text-[0.72rem]">
            <DropdownMenuItem onclick={() => onPull?.()}>Pull</DropdownMenuItem>
            <DropdownMenuItem onclick={() => onPush?.()}>Push</DropdownMenuItem>
            <DropdownMenuItem onclick={() => onFetch?.()}>Fetch</DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onclick={() => onPushForceWithLease?.()}>Force Push (with lease)</DropdownMenuItem>
            {#if hasCommits}
              <DropdownMenuSeparator />
              <DropdownMenuItem destructive onclick={() => onUndoLastCommit?.()}>Undo Last Commit</DropdownMenuItem>
            {/if}
          </DropdownMenuContent>
        </DropdownMenuRoot>
      </ButtonGroup>
    </div>
  </div>

  {#if stagedTree.length === 0 && unstagedTree.length === 0}
    <div class="px-2.5 py-2 text-[0.75rem] text-subtle">No changes.</div>
  {/if}

  {#snippet fileTrailing(node: FileTreeNode<GitChange>)}
    {#if node.change}
      {#if node.change.status !== 'D' && (node.change.additions != null || node.change.deletions != null)}
        {@const net = (node.change.additions ?? 0) - (node.change.deletions ?? 0)}
        {#if net !== 0}
          <span class="shrink-0 pr-2 font-mono {net > 0 ? 'text-success' : 'text-danger'}">
            {net > 0 ? '+' : ''}{net}
          </span>
        {:else}
          <GitStatusBadge status={node.change.status} />
        {/if}
      {:else}
        <GitStatusBadge status={node.change.status} />
      {/if}
    {/if}
  {/snippet}

  {#snippet fileGroup(label: string, tree: FileTreeNode<GitChange>[], keySuffix: string, isStaged: boolean)}
    {#if tree.length > 0}
      <div class="py-1">
        <div class="group/hdr flex items-center px-2.5 py-1">
          <span class="text-[0.68rem] font-medium tracking-wide text-muted uppercase">
            {label} ({countFiles(tree)})
          </span>
          <div class="ml-auto flex items-center gap-0.5 opacity-0 transition-all group-hover/hdr:opacity-100">
            {#if isStaged && onUnstageAll}
              <IconButton
                tooltip="Unstage all"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onUnstageAll?.()}
              >
                <MinusCircle size={13} />
              </IconButton>
            {/if}
            {#if !isStaged && onStageAll}
              <IconButton
                tooltip="Stage all"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onStageAll?.()}
              >
                <PlusCircle size={13} />
              </IconButton>
            {/if}
            {#if !isStaged && onStashAll}
              <IconButton
                tooltip="Stash all"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onStashAll?.()}
              >
                <PackageIcon size={13} />
              </IconButton>
            {/if}
            {#if !isStaged && onDiscardAll}
              <IconButton
                tooltip="Discard all changes"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-danger"
                onclick={() => onDiscardAll?.()}
              >
                <Trash2 size={13} />
              </IconButton>
            {/if}
            {#if onViewAllChanges}
              <IconButton
                tooltip="View all {label.toLowerCase()} diffs"
                tooltipSide="bottom"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => onViewAllChanges?.(isStaged)}
              >
                <FileDiff size={13} />
              </IconButton>
            {/if}
          </div>
        </div>
        <FileTreeItems
          nodes={tree}
          isCollapsed={(path) => collapsedDirs.has(getDirKey(keySuffix, path))}
          onToggleDir={(path) => toggleDir(keySuffix, path)}
          onFileClick={(node) => node.change && onFileClick?.(node.change.path, node.change.staged)}
          onFileDblClick={() => onPersistTab?.()}
          {fileTrailing}
        />
      </div>
    {/if}
  {/snippet}

  {@render fileGroup('Staged', stagedTree, 'staged', true)}
  {@render fileGroup('Changes', unstagedTree, 'unstaged', false)}
</div>
