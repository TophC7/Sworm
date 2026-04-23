<script lang="ts" generics="T extends { path: string }">
  import { TreeNode } from '$lib/components/ui/file-tree'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import type { FileTreeNode } from '$lib/utils/fileTree'
  import type { Snippet } from 'svelte'

  type NodeAttachment = (element: HTMLElement) => void | (() => void)
  const noopAttachment: NodeAttachment = () => () => {}
  const rowActionsHiddenClass = 'group-hover/tree-row:hidden group-has-[:focus-visible]/tree-row:hidden'
  const rowActionsVisibleClass =
    'hidden items-center gap-0.5 group-hover/tree-row:flex group-has-[:focus-visible]/tree-row:flex'

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
    rowActions,
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
    rowActions?: Snippet<[FileTreeNode<T>]>
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
        <div
          class="group/tree-row relative flex min-h-[22px] w-full items-center text-sm transition-colors {dropActive
            ? 'bg-accent/15 text-bright'
            : 'text-muted hover:bg-surface'}"
          role="presentation"
          {@attach targetAttachment ?? noopAttachment}
          oncontextmenu={(e) => {
            if (onFileContextMenu) {
              onFileContextMenu(e, node)
            }
          }}
        >
          <button
            class="relative flex min-h-[22px] min-w-0 flex-1 cursor-pointer items-center gap-1 border-none bg-transparent py-0.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none"
            style="padding-left: {depth * 12 + 10}px"
            draggable={sourceAttachment !== null}
            {@attach sourceAttachment ?? noopAttachment}
            onclick={() => onToggleDir(node.path)}
          >
            {@render indentGuides(depth)}
            <FileIcon filename={node.name} folder expanded={!isCollapsed(node.path)} size={15} />
            <span class="truncate">{node.name}</span>
          </button>
          {#if rowActions || hasDirChanges?.(node.path)}
            <div class="mr-2 ml-1 flex shrink-0 items-center justify-end">
              {#if hasDirChanges?.(node.path)}
                <span class="size-1.5 shrink-0 rounded-full bg-accent {rowActions ? rowActionsHiddenClass : ''}"></span>
              {/if}
              {#if rowActions}
                <div class={rowActionsVisibleClass}>
                  {@render rowActions(node)}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/snippet}
      {#each node.children as child (child.path)}
        {@render renderNode(child, depth + 1)}
      {/each}
    </TreeNode>
  {:else if node.change}
    {@const sourceAttachment = dndEnabled ? (dndSourceAttachment?.(node) ?? null) : null}
    {@const active = isActive?.(node.change.path) ?? false}
    <div
      class="group/tree-row relative flex min-h-[22px] w-full items-center text-sm transition-colors {active
        ? 'bg-accent/10 text-bright'
        : 'text-fg hover:bg-surface'}"
      role="presentation"
      oncontextmenu={(e) => {
        if (onFileContextMenu) {
          onFileContextMenu(e, node)
        }
      }}
    >
      <button
        class="relative flex min-h-[22px] min-w-0 flex-1 cursor-pointer items-center gap-1.5 border-none bg-transparent py-0.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none"
        style="padding-left: {depth * 12 + 10}px"
        draggable={sourceAttachment !== null}
        {@attach sourceAttachment ?? noopAttachment}
        onclick={() => onFileClick?.(node)}
        ondblclick={() => onFileDblClick?.(node)}
      >
        {@render indentGuides(depth)}
        <FileIcon filename={node.path} size={15} />
        <span class="min-w-0 flex-1 truncate">{node.name}</span>
      </button>
      {#if fileTrailing || rowActions}
        <div class="mr-2 ml-1 flex shrink-0 items-center justify-end">
          {#if fileTrailing}
            <div class={rowActions ? rowActionsHiddenClass : ''}>
              {@render fileTrailing(node)}
            </div>
          {/if}
          {#if rowActions}
            <div class={rowActionsVisibleClass}>
              {@render rowActions(node)}
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
{/snippet}

{#each nodes as node (node.path)}
  {@render renderNode(node, 0)}
{/each}
