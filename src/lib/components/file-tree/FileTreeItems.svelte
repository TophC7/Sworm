<script lang="ts" generics="T extends { path: string }">
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import { FixedHeightVirtualList } from '$lib/components/ui/virtual-list'
  import { flattenVisibleTree, type FileTreeNode, type FlatTreeRow } from '$lib/utils/fileTree'
  import type { Snippet } from 'svelte'

  type NodeAttachment = (element: HTMLElement) => void | (() => void)
  const noopAttachment: NodeAttachment = () => () => {}
  const rowActionsHiddenClass = 'group-hover/tree-row:hidden group-has-[:focus-visible]/tree-row:hidden'
  const rowActionsVisibleClass =
    'hidden items-center gap-0.5 group-hover/tree-row:flex group-has-[:focus-visible]/tree-row:flex'
  // Single source of truth for the row's vertical extent. Drives both
  // the virtual list's positioning math and the `style:height` on each
  // row's outer div. Inner buttons inherit via `h-full`.
  const ROW_HEIGHT_PX = 22

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

  // Flatten the visible tree (skipping children of collapsed dirs) so a
  // virtualized renderer can position rows by index. Re-runs on
  // collapse/expand, which is O(n) and only fires on user interaction.
  let rows = $derived<FlatTreeRow<T>[]>(flattenVisibleTree(nodes, isCollapsed))
</script>

<!--
  Indent-guide rules. One 1px column at every 12px depth step,
  rendered as `bg-subtle/25` so they read as faint, non-interactive
  scaffolding rather than a solid border. Reuse this exact recipe in
  any future tree component instead of inventing a different rule
  weight or color.
-->
{#snippet indentGuides(depth: number)}
  {#each Array(depth) as _, i (i)}
    <span class="pointer-events-none absolute top-0 bottom-0 w-px bg-subtle/25" style="left: {i * 12 + 16}px"></span>
  {/each}
{/snippet}

{#snippet directoryRow(node: FileTreeNode<T>, depth: number)}
  {@const sourceAttachment = dndEnabled ? (dndSourceAttachment?.(node) ?? null) : null}
  {@const targetAttachment = dndEnabled ? (dndDirectoryAttachment?.(node) ?? null) : null}
  {@const dropActive = dndEnabled ? (dndIsDropActive?.(node.path) ?? false) : false}
  <div
    class="group/tree-row relative flex w-full items-center text-sm transition-colors {dropActive
      ? 'bg-accent/15 text-bright'
      : 'text-muted hover:bg-surface'}"
    style:height="{ROW_HEIGHT_PX}px"
    role="presentation"
    {@attach targetAttachment ?? noopAttachment}
    oncontextmenu={(e) => onFileContextMenu?.(e, node)}
  >
    <button
      class="relative flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1 border-none bg-transparent py-0.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none"
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

{#snippet fileRow(node: FileTreeNode<T>, depth: number)}
  {@const sourceAttachment = dndEnabled ? (dndSourceAttachment?.(node) ?? null) : null}
  {@const active = node.change ? (isActive?.(node.change.path) ?? false) : false}
  <div
    class="group/tree-row relative flex w-full items-center text-sm transition-colors {active
      ? 'bg-accent/10 text-bright'
      : 'text-fg hover:bg-surface'}"
    style:height="{ROW_HEIGHT_PX}px"
    role="presentation"
    oncontextmenu={(e) => onFileContextMenu?.(e, node)}
  >
    <button
      class="relative flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 border-none bg-transparent py-0.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none"
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
{/snippet}

{#snippet renderRow(row: FlatTreeRow<T>, _index: number)}
  {#if row.node.type === 'directory'}
    {@render directoryRow(row.node, row.depth)}
  {:else if row.node.change}
    {@render fileRow(row.node, row.depth)}
  {/if}
{/snippet}

<FixedHeightVirtualList items={rows} rowHeight={ROW_HEIGHT_PX} row={renderRow} key={(row) => row.node.path} />
