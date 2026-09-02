<!--
  @component
  StatusBarFolderPopover: browse nearby directories and open one as a workbench folder.
-->

<script lang="ts">
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { backend } from '$lib/api/backend'
  import { openFolder } from '$lib/features/workbench/state.svelte'
  import { ArrowUp, FolderOpen } from '$lib/icons/lucideExports'
  import type { FolderInfo } from '$lib/types/backend'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'

  let {
    folderPath,
    children
  }: {
    folderPath: string
    children: import('svelte').Snippet
  } = $props()

  let open = $state(false)
  let directory = $state<FolderInfo | null>(null)
  let folders = $state<FolderInfo[]>([])
  let error = $state<string | null>(null)
  const directoryLoad = createTrackedAsyncLoad<string | null>()
  let loading = $derived(directoryLoad.loading)

  let visibleFolders = $derived(folders.filter((folder) => folder.path !== folderPath))

  function loadDirectory(path: string | null): void {
    directoryLoad.run(path, async (isCurrent) => {
      if (path === null) return
      error = null

      try {
        const resolved = await backend.folders.resolve(path)
        if (!isCurrent()) return
        const entries = await backend.folders.listDirectories(resolved.path)
        if (!isCurrent()) return
        directory = resolved
        folders = entries
      } catch (cause) {
        if (!isCurrent()) return
        error = getErrorMessage(cause)
        folders = []
      }
    })
  }

  function navigateUp() {
    if (!directory) return
    loadDirectory(`${directory.path}/..`)
  }

  async function selectFolder(path: string) {
    open = false
    await openFolder(path)
  }

  $effect(() => {
    const activePath = folderPath
    loadDirectory(open ? `${activePath}/..` : null)
  })
</script>

<DropdownMenuRoot bind:open>
  <DropdownMenuTrigger
    aria-label="Open nearby folder"
    class="cursor-pointer border-none bg-transparent p-0 text-left focus-visible:shadow-focus-ring focus-visible:outline-none"
  >
    {@render children()}
  </DropdownMenuTrigger>

  <DropdownMenuContent class="w-80 p-0" sideOffset={8} align="start">
    <div class="min-w-0 border-b border-edge px-3 py-2">
      <div class="text-xs font-medium text-fg">Open folder</div>
      <div class="truncate font-mono text-2xs text-subtle" title={directory?.path}>{directory?.path ?? folderPath}</div>
    </div>

    <div class="border-b border-edge p-1">
      <DropdownMenuItem
        class="flex items-center gap-2 py-1 text-sm"
        disabled={loading || directory?.path === '/'}
        onclick={(event) => {
          event?.preventDefault()
          void navigateUp()
        }}
      >
        <ArrowUp size={12} class="shrink-0 text-muted" />
        <span class="font-mono">..</span>
      </DropdownMenuItem>
    </div>

    <div class="max-h-72 overflow-y-auto py-1">
      {#if loading}
        <div class="px-3 py-2 text-sm text-subtle">Loading…</div>
      {:else if error}
        <div class="px-3 py-2 text-sm text-danger">{error}</div>
      {:else if visibleFolders.length === 0}
        <div class="px-3 py-2 text-sm text-subtle">No sibling folders.</div>
      {:else}
        {#each visibleFolders as folder (folder.path)}
          <DropdownMenuItem class="flex items-center gap-2 py-1 text-sm" onclick={() => void selectFolder(folder.path)}>
            <FolderOpen size={12} class="shrink-0 text-muted" />
            <span class="min-w-0 flex-1 truncate" title={folder.path}>{folder.name}</span>
          </DropdownMenuItem>
        {/each}
      {/if}
    </div>
  </DropdownMenuContent>
</DropdownMenuRoot>
