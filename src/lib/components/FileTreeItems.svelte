<script lang="ts" generics="T extends { path: string }">
  import { TreeNode } from '$lib/components/ui/file-tree'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import type { FileTreeNode } from '$lib/utils/fileTree'
  import type { Snippet } from 'svelte'

  let {
    nodes,
    isCollapsed,
    onToggleDir,
    onFileClick,
    onFileDblClick,
    fileTrailing
  }: {
    nodes: FileTreeNode<T>[]
    isCollapsed: (path: string) => boolean
    onToggleDir: (path: string) => void
    onFileClick?: (node: FileTreeNode<T>) => void
    onFileDblClick?: (node: FileTreeNode<T>) => void
    fileTrailing?: Snippet<[FileTreeNode<T>]>
  } = $props()
</script>

{#snippet indentGuides(depth: number)}
  {#each Array(depth) as _, i (i)}
    <span class="pointer-events-none absolute top-0 bottom-0 w-px bg-subtle/25" style="left: {i * 12 + 16}px"></span>
  {/each}
{/snippet}

{#snippet renderNode(node: FileTreeNode<T>, depth: number)}
  {#if node.type === 'directory'}
    <TreeNode expanded={!isCollapsed(node.path)} {depth}>
      {#snippet label()}
        <button
          class="relative flex w-full cursor-pointer items-center gap-1 border-none bg-transparent py-0.5 text-left text-[0.72rem] text-muted hover:bg-surface"
          style="padding-left: {depth * 12 + 10}px"
          onclick={() => onToggleDir(node.path)}
        >
          {@render indentGuides(depth)}
          <FileIcon filename={node.name} folder expanded={!isCollapsed(node.path)} size={14} />
          <span class="truncate">{node.name}</span>
        </button>
      {/snippet}
      {#each node.children as child (child.path)}
        {@render renderNode(child, depth + 1)}
      {/each}
    </TreeNode>
  {:else if node.change}
    <button
      class="relative flex w-full cursor-pointer items-center gap-1.5 border-none bg-transparent py-0.5 text-left text-[0.75rem] text-fg hover:bg-surface"
      style="padding-left: {depth * 12 + 10}px"
      onclick={() => onFileClick?.(node)}
      ondblclick={() => onFileDblClick?.(node)}
    >
      {@render indentGuides(depth)}
      <FileIcon filename={node.name} size={14} />
      <span class="min-w-0 flex-1 truncate">{node.name}</span>
      {#if fileTrailing}
        {@render fileTrailing(node)}
      {/if}
    </button>
  {/if}
{/snippet}

{#each nodes as node (node.path)}
  {@render renderNode(node, 0)}
{/each}
