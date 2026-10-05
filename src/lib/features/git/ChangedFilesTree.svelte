<script lang="ts">
  import type { TabId } from '$lib/features/workbench/model'
  import type { FileDiff } from '$lib/types/backend'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import FileTreeItems from '$lib/components/file-tree/FileTreeItems.svelte'
  import GitStatusBadge from '$lib/features/git/GitStatusBadge.svelte'
  import { promoteTabWhenReady } from '$lib/features/workbench/state.svelte'
  import { SvelteSet } from 'svelte/reactivity'

  let { files, open }: { files: FileDiff[]; open: (path: string) => Promise<TabId> } = $props()
  let tree = $derived(buildFileTree(files))
  const collapsedDirs = new SvelteSet<string>()
  let pending = $state<Promise<TabId> | null>(null)
</script>

{#if tree.length === 0}
  <div class="px-4 py-1.5 text-xs text-subtle">No files changed.</div>
{:else}
  <FileTreeItems
    nodes={tree}
    isCollapsed={(node) => collapsedDirs.has(node.path)}
    onToggleDir={(path) => {
      if (collapsedDirs.has(path)) collapsedDirs.delete(path)
      else collapsedDirs.add(path)
    }}
    onFileClick={(node) => {
      if (node.change) pending = open(node.change.path)
    }}
    onFileDblClick={() => promoteTabWhenReady(pending)}
  >
    {#snippet fileTrailing(node: FileTreeNode<FileDiff>)}
      {#if node.change}
        <GitStatusBadge status={node.change.status} />
      {/if}
    {/snippet}
  </FileTreeItems>
{/if}
