<script lang="ts">
  import { IconButton } from '$lib/components/ui/button'
  import { openBrowser } from '$lib/features/browser/state.svelte'
  import { getActiveFolderPath, openNewTab } from '$lib/features/workbench/state.svelte'
  import { Plus } from '$lib/icons/lucideExports'

  function handleNewTab(): void {
    const folderPath = getActiveFolderPath()
    if (folderPath) openNewTab(folderPath)
    else openBrowser()
  }
</script>

<IconButton size="md" tooltip="New Tab" class="sticky right-0 ml-0.5 shrink-0 bg-surface" onclick={handleNewTab}>
  <Plus size={14} />
</IconButton>
