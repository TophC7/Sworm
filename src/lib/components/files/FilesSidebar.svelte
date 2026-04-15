<script lang="ts">
  import { SvelteSet } from 'svelte/reactivity'
  import { buildFileTree, type FileTreeNode } from '$lib/utils/fileTree'
  import FileTreeItems from '$lib/components/FileTreeItems.svelte'
  import { Button } from '$lib/components/ui/button'
  import { backend } from '$lib/api/backend'
  import { PanelLeftClose } from '$lib/icons/lucideExports'
  import { RotateCw } from '$lib/icons/lucideExports'
  import { setGitSidebarCollapsed } from '$lib/stores/ui.svelte'
  import { openFile } from '$lib/utils/openFile'

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

<div class="flex h-full flex-col bg-ground">
  <!-- Header -->
  <div class="flex h-8 min-h-8 shrink-0 items-center justify-between border-b border-edge bg-surface px-2.5">
    <div class="flex items-center gap-1.5">
      <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Files</span>
      <Button variant="ghost" size="icon-sm" onclick={loadFiles} title="Refresh">
        <RotateCw size={11} />
      </Button>
    </div>
    <Button variant="ghost" size="icon-sm" onclick={() => setGitSidebarCollapsed(true)}>
      <PanelLeftClose size={12} />
    </Button>
  </div>

  <!-- Content -->
  <div class="flex-1 overflow-y-auto text-[0.78rem]">
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
      />
    {/if}
  </div>
</div>
