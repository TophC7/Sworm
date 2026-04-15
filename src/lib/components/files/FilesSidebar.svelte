<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import SidebarPanel from '$lib/components/SidebarPanel.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { backend } from '$lib/api/backend'
  import { getGitSummary } from '$lib/stores/git.svelte'
  import GitStatusBadge from '$lib/components/git/GitStatusBadge.svelte'
  import { RotateCw, SquareArrowOutUpRight } from '$lib/icons/lucideExports'
  import { openFile } from '$lib/utils/openFile'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'

  let {
    projectId,
    projectPath
  }: {
    projectId: string
    projectPath: string
  } = $props()

  let expandedDirs = new SvelteSet<string>()
  let loading = $state(true)
  let error = $state<string | null>(null)
  let fileTree = $state<FileTreeNode<{ path: string }>[]>([])

  async function loadFiles() {
    loading = true
    error = null
    try {
      const paths = await backend.files.listAll(projectPath)
      fileTree = buildFileTree(paths.map((p) => ({ path: p })))
    } catch (e) {
      console.error('Failed to load files:', e)
      error = e instanceof Error ? e.message : String(e)
    } finally {
      loading = false
    }
  }

  function toggleDir(path: string) {
    if (expandedDirs.has(path)) expandedDirs.delete(path)
    else expandedDirs.add(path)
  }

  function handleFileClick(filePath: string) {
    openFile(projectId, projectPath, filePath).catch((e) => console.error('Failed to open file:', e))
  }

  // Path → status letter lookup from git state (prefer unstaged over staged)
  let gitSummary = $derived(getGitSummary(projectId))
  let gitStatusMap = $derived.by(() => {
    const map = new Map<string, string>()
    if (!gitSummary?.changes) return map
    for (const change of gitSummary.changes) {
      if (!map.has(change.path) || !change.staged) {
        map.set(change.path, change.status)
      }
    }
    return map
  })

  // Reload file list when project changes
  let prevProjectPath = ''
  $effect(() => {
    if (projectPath !== prevProjectPath) {
      prevProjectPath = projectPath
      expandedDirs.clear()
      loadFiles()
    }
  })
</script>

<SidebarPanel title="Files">
  {#snippet headerActions()}
    <IconButton tooltip="Refresh files" onclick={loadFiles}>
      <RotateCw size={11} />
    </IconButton>
    <IconButton tooltip="Open in file manager" onclick={() => revealItemInDir(projectPath)}>
      <SquareArrowOutUpRight size={11} />
    </IconButton>
  {/snippet}

  <div class="h-full overflow-y-auto text-[0.78rem]">
    {#if loading}
      <div class="px-2.5 py-3 text-[0.75rem] text-subtle">Loading files&hellip;</div>
    {:else if error}
      <div class="px-2.5 py-3 text-[0.75rem] text-danger">{error}</div>
    {:else if fileTree.length === 0}
      <div class="px-2.5 py-3 text-[0.75rem] text-subtle">No files found.</div>
    {:else}
      <FileTreeItems
        nodes={fileTree}
        isCollapsed={(path) => !expandedDirs.has(path)}
        onToggleDir={toggleDir}
        onFileClick={(node) => {
          if (node.change?.path) handleFileClick(node.change.path)
        }}
      >
        {#snippet fileTrailing(node)}
          {@const status = gitStatusMap.get(node.path)}
          {#if status}
            <GitStatusBadge {status} />
          {/if}
        {/snippet}
      </FileTreeItems>
    {/if}
  </div>
</SidebarPanel>
