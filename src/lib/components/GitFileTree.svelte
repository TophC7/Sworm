<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import type { GitChange, GitSummary } from '$lib/types/backend'
  import { buildFileTree, countFiles, type FileTreeNode } from '$lib/utils/fileTree'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import FileDiff from '@lucide/svelte/icons/file-diff'
  import { gitStatusColor, gitStatusDisplay, gitStatusLabel } from '$lib/utils/gitStatus'

  let {
    summary,
    projectPath,
    onFileClick,
    onPersistTab,
    onViewAllChanges
  }: {
    summary: GitSummary
    projectPath: string
    onFileClick?: (filePath: string, staged: boolean) => void
    onPersistTab?: () => void
    onViewAllChanges?: (staged: boolean) => void
  } = $props()

  let collapsedDirs = new SvelteSet<string>()

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
</script>

<div class="text-[0.78rem]">
  {#if stagedTree.length === 0 && unstagedTree.length === 0}
    <div class="px-2.5 py-2 text-[0.75rem] text-subtle">No changes.</div>
  {/if}

  {#snippet fileTrailing(node: FileTreeNode<GitChange>)}
    {#if node.change}
      {#if node.change.status !== 'D' && (node.change.additions != null || node.change.deletions != null)}
        {@const net = (node.change.additions ?? 0) - (node.change.deletions ?? 0)}
        {#if net !== 0}
          <span class="shrink-0 font-mono {net > 0 ? 'text-success' : 'text-danger'}">
            {net > 0 ? '+' : ''}{net}
          </span>
        {:else}
          <span
            class="w-3.5 shrink-0 text-center font-mono font-bold {gitStatusColor(node.change.status)}"
            title={gitStatusLabel(node.change.status)}>{gitStatusDisplay(node.change.status)}</span
          >
        {/if}
      {:else}
        <span
          class="w-3.5 shrink-0 text-center font-mono font-bold {gitStatusColor(node.change.status)}"
          title={gitStatusLabel(node.change.status)}>{gitStatusDisplay(node.change.status)}</span
        >
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
          {#if onViewAllChanges}
            <button
              class="ml-auto rounded p-0.5 text-muted opacity-0 transition-all group-hover/hdr:opacity-100 hover:text-fg"
              onclick={() => onViewAllChanges?.(isStaged)}
              title="View all {label.toLowerCase()} diffs"
            >
              <FileDiff size={13} />
            </button>
          {/if}
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
