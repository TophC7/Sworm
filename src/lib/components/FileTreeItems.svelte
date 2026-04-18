<script lang="ts" generics="T extends { path: string }">
  import { TreeNode } from '$lib/components/ui/file-tree'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import type { FileTreeNode } from '$lib/utils/fileTree'
  import type { Snippet } from 'svelte'

  type NodeAttachment = (element: HTMLElement) => void | (() => void)
  const noopAttachment: NodeAttachment = () => () => {}

  let {
    nodes,
    isCollapsed,
    isActive,
    hasDirChanges,
    onToggleDir,
    onFileClick,
    onFileDblClick,
    onFileContextMenu,
    fileTrailing,
    dndEnabled = false,
    dndSourceAttachment,
    dndDirectoryAttachment,
    dndIsDropActive
  }: {
    nodes: FileTreeNode<T>[]
    isCollapsed: (path: string) => boolean
    isActive?: (path: string) => boolean
    hasDirChanges?: (dirPath: string) => boolean
    onToggleDir: (path: string) => void
    onFileClick?: (node: FileTreeNode<T>) => void
    onFileDblClick?: (node: FileTreeNode<T>) => void
    onFileContextMenu?: (e: MouseEvent, node: FileTreeNode<T>) => void
    fileTrailing?: Snippet<[FileTreeNode<T>]>
    dndEnabled?: boolean
    dndSourceAttachment?: (node: FileTreeNode<T>) => NodeAttachment | null
    dndDirectoryAttachment?: (node: FileTreeNode<T>) => NodeAttachment | null
    dndIsDropActive?: (path: string) => boolean
  } = $props()
</script>

{#snippet indentGuides(depth: number)}
  {#each Array(depth) as _, i (i)}
    <span class="pointer-events-none absolute top-0 bottom-0 w-px bg-subtle/25" style="left: {i * 12 + 16}px"></span>
  {/each}
{/snippet}

{#snippet renderNode(node: FileTreeNode<T>, depth: number)}
  {#if node.type === 'directory'}
    {@const sourceAttachment = dndEnabled ? (dndSourceAttachment?.(node) ?? null) : null}
    {@const targetAttachment = dndEnabled ? (dndDirectoryAttachment?.(node) ?? null) : null}
    {@const dropActive = dndEnabled ? (dndIsDropActive?.(node.path) ?? false) : false}
    <TreeNode expanded={!isCollapsed(node.path)} {depth}>
      {#snippet label()}
        <button
          class="relative flex w-full cursor-pointer items-center gap-1 border-none py-0.5 text-left text-[0.72rem] text-muted hover:bg-surface {dropActive
            ? 'bg-accent/14 text-bright'
            : 'bg-transparent'}"
          style="padding-left: {depth * 12 + 10}px"
          draggable={sourceAttachment !== null}
          {@attach sourceAttachment ?? noopAttachment}
          {@attach targetAttachment ?? noopAttachment}
          onclick={() => onToggleDir(node.path)}
          oncontextmenu={(e) => {
            if (onFileContextMenu) {
              onFileContextMenu(e, node)
            }
          }}
        >
          {@render indentGuides(depth)}
          <FileIcon filename={node.name} folder expanded={!isCollapsed(node.path)} size={14} />
          <span class="truncate">{node.name}</span>
          {#if hasDirChanges?.(node.path)}
            <span class="mr-2 ml-auto size-1.5 shrink-0 rounded-full bg-accent"></span>
          {/if}
        </button>
      {/snippet}
      {#each node.children as child (child.path)}
        {@render renderNode(child, depth + 1)}
      {/each}
    </TreeNode>
  {:else if node.change}
    {@const sourceAttachment = dndEnabled ? (dndSourceAttachment?.(node) ?? null) : null}
    {@const active = isActive?.(node.change.path) ?? false}
    <button
      class="relative flex w-full cursor-pointer items-center gap-1.5 border-none py-0.5 text-left text-[0.75rem] hover:bg-surface {active
        ? 'bg-accent/10 text-bright'
        : 'bg-transparent text-fg'}"
      style="padding-left: {depth * 12 + 10}px"
      draggable={sourceAttachment !== null}
      {@attach sourceAttachment ?? noopAttachment}
      onclick={() => onFileClick?.(node)}
      ondblclick={() => onFileDblClick?.(node)}
      oncontextmenu={(e) => {
        if (onFileContextMenu) {
          onFileContextMenu(e, node)
        }
      }}
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
